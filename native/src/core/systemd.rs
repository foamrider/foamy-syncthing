//! One explicitly authorized systemd user unit (Syncshell
//! `internal/systemduser`). The core runs exactly
//! `systemctl --user show <unit> --no-pager --property=LoadState
//! --property=ActiveState --property=UnitFileState --property=FragmentPath
//! --property=ExecStart --property=Environment` (3 s) and
//! `systemctl --user start|stop <unit>` (10 s), with stdin `/dev/null`,
//! output captured and capped at 64 KiB and never shown, and only for a
//! unit named on the command line with `--lifecycle-authorized`.

use std::collections::BTreeMap;
use std::time::Duration;

use foamy_helper::child::Bounded;

use super::gocompat::{clean, join};
use super::types::{Lifecycle, bounded};

const MAX_SHOW_OUTPUT: usize = 64 << 10;
const SHOW_TIMEOUT: Duration = Duration::from_secs(3);
const ACTION_TIMEOUT: Duration = Duration::from_secs(10);
/// Linux `PATH_MAX`.
const MAX_PATH_BYTES: usize = 4096;

/// Host authority for one exact unit.
#[derive(Clone, Debug, Default)]
pub struct Binding {
    pub authorized: bool,
    pub unit: String,
    pub config_path: String,
}

/// Nonsecret facts about the selected target.
#[derive(Clone, Debug, Default)]
pub struct Target {
    pub config_path: String,
    pub local: bool,
    pub automatic: bool,
}

/// `^[A-Za-z0-9_.@:-]+[.]service$`
pub fn valid_unit(unit: &str) -> bool {
    let Some(stem) = unit.strip_suffix(".service") else {
        return false;
    };
    !stem.is_empty()
        && stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.@:-".contains(&b))
}

/// `start` or `stop` of an already authorized unit.
pub fn apply(unit: &str, action: &str) -> Result<(), String> {
    if !valid_unit(unit) {
        return Err("lifecycle unit is invalid".into());
    }
    if action != "start" && action != "stop" {
        return Err("lifecycle action is invalid".into());
    }
    let failed = || "authorized user-service action failed".to_string();
    let output = Bounded::new("systemctl")
        .args(["--user", action, unit])
        .timeout(ACTION_TIMEOUT)
        .max_output(MAX_SHOW_OUTPUT)
        .run()
        .map_err(|_| failed())?;
    if !output.success() || output.stdout.len() + output.stderr.len() > MAX_SHOW_OUTPUT {
        return Err(failed());
    }
    Ok(())
}

fn show(unit: &str) -> Option<BTreeMap<String, String>> {
    let output = Bounded::new("systemctl")
        .args([
            "--user",
            "show",
            unit,
            "--no-pager",
            "--property=LoadState",
            "--property=ActiveState",
            "--property=UnitFileState",
            "--property=FragmentPath",
            "--property=ExecStart",
            "--property=Environment",
        ])
        .timeout(SHOW_TIMEOUT)
        .max_output(MAX_SHOW_OUTPUT)
        .run()
        .ok()?;
    if !output.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut properties = BTreeMap::new();
    for line in text.split('\n') {
        if let Some((key, value)) = line.split_once('=') {
            properties.insert(key.to_string(), value.trim().to_string());
        }
    }
    Some(properties)
}

/// Observes the unit after the API state is known.
pub fn probe(binding: &Binding, target: &Target, api_online: bool) -> Lifecycle {
    let mut state = Lifecycle {
        kind: "systemd-user".into(),
        unit: binding.unit.clone(),
        classification: "external".into(),
        authorized: binding.authorized,
        ..Lifecycle::default()
    };
    if !binding.authorized || !valid_unit(&binding.unit) {
        return state;
    }
    let Some(properties) = show(&binding.unit) else {
        state.error = "could not inspect authorized user service".into();
        return state;
    };
    let get = |key: &str| properties.get(key).map(String::as_str).unwrap_or("");
    state.active_state = bounded(get("ActiveState"), 64);
    state.unit_file_state = bounded(get("UnitFileState"), 64);
    state.available = get("LoadState") == "loaded" && !get("FragmentPath").is_empty();
    state.active = matches!(
        state.active_state.as_str(),
        "active" | "activating" | "reloading"
    );
    state.target_match = state.available && target_matches(binding, target, &properties);
    if api_online && (!state.active || !state.target_match) {
        state.classification = "external".into();
        return state;
    }
    if state.active {
        state.classification = if api_online {
            "managed"
        } else {
            "unit-active-api-unavailable"
        }
        .into();
        state.can_control = state.target_match;
        return state;
    }
    if state.target_match {
        state.classification = "stopped-candidate".into();
        state.can_control = true;
        state.can_start = true;
    }
    state
}

fn target_matches(
    binding: &Binding,
    target: &Target,
    properties: &BTreeMap<String, String>,
) -> bool {
    if !target.local || target.config_path.is_empty() {
        return false;
    }
    let selected = clean(&target.config_path);
    if !binding.config_path.is_empty() && clean(&binding.config_path) != selected {
        return false;
    }
    let get = |key: &str| properties.get(key).map(String::as_str).unwrap_or("");
    let invocation = format!("{} {}", get("ExecStart"), get("Environment"));
    if let Some(matched) = invocation_names(&invocation, &selected) {
        return matched;
    }
    if !binding.config_path.is_empty() {
        return false;
    }
    target.automatic
}

/// Whether the config directory an invocation names (`--config`, `--home`,
/// `-C`, `-H`, with a space, `=` or nothing; `STCONFDIR=`/`STHOMEDIR=`) holds
/// `selected`; `None` when it names none. The first option decides.
/// `systemctl show` joins ExecStart's argv with spaces and no quoting, and
/// quotes an Environment entry that holds a space, so a directory with a
/// space spans several words. The value runs to the closing quote when it is
/// quoted; else an Environment value is one word, and an option's value ends
/// before `;` (the end of argv), a quote, an option (`-`) or the end. The
/// whole value must be the selected directory: a unit for `/a/b c` never
/// controls `/a/b`. A directory word after a space that starts with `-` ends
/// the value early, so such a unit matches nothing.
fn invocation_names(invocation: &str, selected: &str) -> Option<bool> {
    let replaced = invocation.replace([';', '"'], " ");
    // Byte spans of the words, so a value that spans words keeps its spacing.
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (index, c) in replaced.char_indices().chain([(replaced.len(), ' ')]) {
        match (start, c.is_whitespace()) {
            (None, false) => start = Some(index),
            (Some(from), true) => {
                words.push((from, index));
                start = None;
            }
            _ => {}
        }
    }
    let (value, first) = words.iter().enumerate().find_map(|(index, &(from, to))| {
        let field = &replaced[from..to];
        if matches!(field, "--config" | "--home" | "-C" | "-H") && index + 1 < words.len() {
            return Some((words[index + 1].0, index + 1));
        }
        [
            "--config=",
            "--home=",
            "-C=",
            "-H=",
            "-C",
            "-H",
            "STCONFDIR=",
            "STHOMEDIR=",
        ]
        .into_iter()
        .find(|prefix| field.starts_with(prefix))
        .map(|prefix| (from + prefix.len(), index))
    })?;
    // `replaced` has the same byte offsets as `invocation`. Before `value`
    // in its word is the option's prefix, if any.
    let opened = words[first].0;
    let quoted = invocation[..opened].ends_with('"');
    let environment = replaced[opened..value].starts_with("ST");
    let ends_before = |to: usize| match invocation[to..].chars().find(|c| !c.is_whitespace()) {
        None => true,
        Some(next) if quoted => next == '"',
        Some(next) => environment || matches!(next, ';' | '"' | '-'),
    };
    // No directory is longer than PATH_MAX, which bounds the work on a long
    // invocation.
    let end = words[first..]
        .iter()
        .map(|&(_, to)| to)
        .take_while(|&to| to - value <= MAX_PATH_BYTES)
        .find(|&to| ends_before(to));
    Some(end.is_some_and(|to| clean(&join(&[&replaced[value..to], "config.xml"])) == selected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_are_validated_exactly() {
        assert!(valid_unit("syncthing.service"));
        assert!(valid_unit("syncthing@me:x-1_.service"));
        for bad in [
            "../bad.service",
            ".service",
            "syncthing",
            "a b.service",
            "syncthing.service\n",
            "x.service.d",
        ] {
            assert!(!valid_unit(bad), "{bad}");
        }
    }

    /// `systemctl show`'s ExecStart for a unit that runs `syncthing serve
    /// --home "<home>" --no-browser`: argv joined by spaces, unquoted.
    fn exec_start(home: &str) -> String {
        format!(
            "{{ path=/usr/bin/syncthing ; argv[]=/usr/bin/syncthing serve --home {home} \
             --no-browser ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; \
             code=(null) ; status=0/0 }}"
        )
    }

    #[test]
    fn invocations_name_their_config_directory() {
        let names = |invocation: &str, directory: &str| {
            invocation_names(invocation, &join(&[directory, "config.xml"]))
        };
        let long = format!("/{}", "a".repeat(MAX_PATH_BYTES));
        for (invocation, directory) in [
            (
                "{ argv[]=/usr/bin/syncthing serve --home /d ; }".to_string(),
                "/d",
            ),
            ("syncthing --config=/d/".into(), "/d"),
            ("syncthing -C/d".into(), "/d"),
            ("syncthing -H=/d".into(), "/d"),
            ("\"syncthing\" \"-H\" \"/d\"".into(), "/d"),
            ("/usr/bin/syncthing serve STHOMEDIR=/e".into(), "/e"),
            ("syncthing --config=".into(), ""),
            // A directory with spaces spans words, with its own spacing.
            (exec_start("/home/me/My Sync"), "/home/me/My Sync"),
            (exec_start("/home/me/My  Sync/"), "/home/me/My  Sync"),
            ("syncthing serve --home=/m/A B C".into(), "/m/A B C"),
            (
                "\"STHOMEDIR=/home/me/My Sync\" STNORESTART=1".into(),
                "/home/me/My Sync",
            ),
            ("STHOMEDIR=/d STNORESTART=1".into(), "/d"),
            ("syncthing --home /d;".into(), "/d"),
        ] {
            assert_eq!(names(&invocation, directory), Some(true), "{invocation}");
        }
        for (invocation, directory) in [
            (
                "{ argv[]=/usr/bin/syncthing serve --home /d ; }".to_string(),
                "/e",
            ),
            ("syncthing --config=".into(), "/d"),
            (exec_start("/home/me/My Sync"), "/home/me/My Sync/x"),
            (exec_start("/home/me/My Sync"), "/home/me/My  Sync"),
            // The whole value is compared, not a part of it before a space.
            (exec_start("/home/me/My Sync Other"), "/home/me/My Sync"),
            (exec_start("/home/me/My Sync"), "/home/me/My"),
            ("syncthing serve --home=/m/A B C".into(), "/m/A B"),
            (
                "\"STHOMEDIR=/home/me/My Sync Other\" STNORESTART=1".into(),
                "/home/me/My Sync",
            ),
            ("STHOMEDIR=/d STNORESTART=1".into(), "/d STNORESTART=1"),
            // The first option decides.
            ("syncthing --home /d STHOMEDIR=/e".into(), "/e"),
            // Nothing longer than PATH_MAX is compared.
            (format!("syncthing --home {long}"), long.as_str()),
        ] {
            assert_eq!(names(&invocation, directory), Some(false), "{invocation}");
        }
        for invocation in ["syncthing --home", "syncthing serve --no-browser"] {
            assert_eq!(names(invocation, "/d"), None, "{invocation}");
        }
    }

    #[test]
    fn matching_needs_a_local_target_and_the_same_config() {
        let properties = |exec: &str| BTreeMap::from([("ExecStart".to_string(), exec.to_string())]);
        let binding = Binding {
            authorized: true,
            unit: "syncthing.service".into(),
            config_path: String::new(),
        };
        let target = Target {
            config_path: "/s/config.xml".into(),
            local: true,
            automatic: true,
        };
        assert!(target_matches(
            &binding,
            &target,
            &properties("syncthing serve")
        ));
        assert!(target_matches(
            &binding,
            &target,
            &properties("syncthing -H /s/")
        ));
        assert!(!target_matches(
            &binding,
            &target,
            &properties("syncthing -H /t")
        ));
        let flatpak = Target {
            automatic: false,
            ..target.clone()
        };
        assert!(!target_matches(
            &binding,
            &flatpak,
            &properties("syncthing serve")
        ));
        let remote = Target {
            local: false,
            ..target.clone()
        };
        assert!(!target_matches(
            &binding,
            &remote,
            &properties("syncthing -H /s")
        ));
        let bound = Binding {
            config_path: "/s/config.xml".into(),
            ..binding.clone()
        };
        assert!(!target_matches(
            &bound,
            &target,
            &properties("syncthing serve")
        ));
        assert!(target_matches(
            &bound,
            &target,
            &properties("syncthing --home=/s")
        ));
        // A home with a space, as `systemctl show` prints the unit.
        let spaced = Target {
            config_path: "/home/me/My Sync/config.xml".into(),
            ..target.clone()
        };
        assert!(target_matches(
            &binding,
            &spaced,
            &properties(&exec_start("/home/me/My Sync"))
        ));
        let environment = BTreeMap::from([
            (
                "ExecStart".to_string(),
                exec_start("").replace("--home ", ""),
            ),
            (
                "Environment".to_string(),
                "\"STHOMEDIR=/home/me/My Sync\" STNORESTART=1".to_string(),
            ),
        ]);
        assert!(target_matches(&binding, &spaced, &environment));
        assert!(!target_matches(
            &binding,
            &spaced,
            &properties(&exec_start("/home/me/My"))
        ));
        assert!(!target_matches(
            &binding,
            &spaced,
            &properties(&exec_start("/home/me/My Sync Other"))
        ));
    }
}
