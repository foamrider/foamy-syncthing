//! One explicitly authorized systemd user unit (Syncshell
//! `internal/systemduser`). The core runs exactly
//! `systemctl --user show <unit> --no-pager --property=LoadState
//! --property=ActiveState --property=UnitFileState --property=FragmentPath`
//! and `busctl --user --json=short get-property org.freedesktop.systemd1
//! <unit object path> org.freedesktop.systemd1.Service ExecStart Environment`
//! (3 s each) and `systemctl --user start|stop <unit>` (10 s), with stdin
//! `/dev/null`, output captured and capped at 64 KiB and never shown, and
//! only for a unit named on the command line with `--lifecycle-authorized`.
//! The unit's argv and environment come from the bus, not from `systemctl
//! show` as in Syncshell: `systemctl show` joins argv with spaces and drops
//! the unit file's quotes, so its text can't say where a directory with a
//! space ends. Only a unit that runs syncthing itself, with no `$` in its
//! arguments, can match: a wrapper (a shell, env, a script) keeps the
//! configuration in a string or a file that the core does not parse, and
//! systemd expands `$NAME`, `${NAME}` and `$$` only when it starts the unit.

use std::collections::BTreeMap;
use std::time::Duration;

use foamy_helper::child::Bounded;
use serde::Deserialize;

use super::gocompat::{base, clean, join};
use super::types::{Lifecycle, bounded};

const MAX_SHOW_OUTPUT: usize = 64 << 10;
const SHOW_TIMEOUT: Duration = Duration::from_secs(3);
const ACTION_TIMEOUT: Duration = Duration::from_secs(10);

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

/// The unit's `ExecStart` commands, each the program systemd runs and its
/// argv, and its `Environment` entries; every argument whole.
#[derive(Debug, Default, PartialEq)]
struct Invocation {
    commands: Vec<(String, Vec<String>)>,
    environment: Vec<String>,
}

/// The unit's invocation as systemd's bus holds it.
fn invocation(unit: &str) -> Option<Invocation> {
    let path = unit_object_path(unit);
    let output = Bounded::new("busctl")
        .args([
            "--user",
            "--json=short",
            "get-property",
            "org.freedesktop.systemd1",
            path.as_str(),
            "org.freedesktop.systemd1.Service",
            "ExecStart",
            "Environment",
        ])
        .timeout(SHOW_TIMEOUT)
        .max_output(MAX_SHOW_OUTPUT)
        .run()
        .ok()?;
    if !output.success() {
        return None;
    }
    parse_invocation(&output.stdout)
}

/// The unit's object path on systemd's bus (`sd_bus_path_encode`): each
/// byte but a letter, or a digit after the first byte, as `_` and two
/// lowercase hex digits.
fn unit_object_path(unit: &str) -> String {
    let mut path = String::from("/org/freedesktop/systemd1/unit/");
    for (index, byte) in unit.bytes().enumerate() {
        if byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit()) {
            path.push(char::from(byte));
        } else {
            path.push_str(&format!("_{byte:02x}"));
        }
    }
    path
}

/// One property as `busctl --json=short` prints it.
#[derive(Deserialize)]
struct Property<T> {
    #[serde(rename = "type")]
    signature: String,
    data: T,
}

/// One `ExecStart` command, D-Bus `(sasbttttuii)`: the path, argv, whether a
/// failure is ignored, start and exit times (realtime, monotonic), the pid,
/// the exit code and the status.
type ExecCommand = (String, Vec<String>, bool, u64, u64, u64, u64, u32, i32, i32);

/// busctl's reply: one line per property, `ExecStart` and `Environment` in
/// that order. Any other shape is refused.
fn parse_invocation(stdout: &[u8]) -> Option<Invocation> {
    let text = std::str::from_utf8(stdout).ok()?;
    let mut lines = text.lines();
    let commands: Property<Vec<ExecCommand>> = serde_json::from_str(lines.next()?).ok()?;
    let environment: Property<Vec<String>> = serde_json::from_str(lines.next()?).ok()?;
    if lines.next().is_some()
        || commands.signature != "a(sasbttttuii)"
        || environment.signature != "as"
    {
        return None;
    }
    Some(Invocation {
        commands: commands
            .data
            .into_iter()
            .map(|command| (command.0, command.1))
            .collect(),
        environment: environment.data,
    })
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
    if state.available {
        match target_matches(binding, target, || invocation(&binding.unit)) {
            Some(matched) => state.target_match = matched,
            None => state.error = "could not inspect authorized user service".into(),
        }
    }
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

/// Whether the unit runs the selected Syncthing; `None` when its argv could
/// not be read. `invocation` reads it, only when a local target needs it.
fn target_matches(
    binding: &Binding,
    target: &Target,
    invocation: impl FnOnce() -> Option<Invocation>,
) -> Option<bool> {
    if !target.local || target.config_path.is_empty() {
        return Some(false);
    }
    let selected = clean(&target.config_path);
    if !binding.config_path.is_empty() && clean(&binding.config_path) != selected {
        return Some(false);
    }
    let invocation = invocation()?;
    // Only syncthing's own argv, as systemd passes it on, says which
    // configuration it runs. A wrapper (`sh -c '…'`, env, a script) holds it
    // where it isn't parsed here. systemd replaces `$NAME` and `${NAME}` only
    // when it starts the command, and turns `$$` into `$` unless the command
    // has the `:` prefix, which the bus doesn't show: an argument with a `$`
    // has no known value. Such a unit matches nothing, also when its strings
    // name this configuration.
    let known = |(program, argv): &(String, Vec<String>)| {
        base(program) == "syncthing" && !argv.iter().any(|argument| argument.contains('$'))
    };
    if invocation.commands.is_empty() || !invocation.commands.iter().all(known) {
        return Some(false);
    }
    let lists = invocation.commands.iter().map(|(_, argv)| argv.as_slice());
    if let Some(configured) =
        invocation_config_path(lists.chain([invocation.environment.as_slice()]))
    {
        return Some(clean(&configured) == selected);
    }
    if !binding.config_path.is_empty() {
        return Some(false);
    }
    Some(target.automatic)
}

/// The config directory an invocation names (`--config`, `--home`, `-C`,
/// `-H`, with the value as the next argument, after `=` or right after the
/// option; `STCONFDIR=`/`STHOMEDIR=`), as `<dir>/config.xml`. Each value is
/// one whole argument, spaces included, and each list (a command's argv, the
/// environment) is read on its own: an option that ends a list takes no
/// value from the next.
fn invocation_config_path<'a>(lists: impl IntoIterator<Item = &'a [String]>) -> Option<String> {
    for fields in lists {
        for (index, field) in fields.iter().enumerate() {
            if matches!(field.as_str(), "--config" | "--home" | "-C" | "-H")
                && index + 1 < fields.len()
            {
                return Some(join(&[fields[index + 1].as_str(), "config.xml"]));
            }
            for prefix in [
                "--config=",
                "--home=",
                "-C=",
                "-H=",
                "-C",
                "-H",
                "STCONFDIR=",
                "STHOMEDIR=",
            ] {
                if let Some(rest) = field.strip_prefix(prefix) {
                    return Some(join(&[rest, "config.xml"]));
                }
            }
        }
    }
    None
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

    /// Argument lists from string slices.
    fn lists(lists: &[&[&str]]) -> Vec<Vec<String>> {
        lists
            .iter()
            .map(|list| list.iter().map(|field| field.to_string()).collect())
            .collect()
    }

    /// A unit whose one command runs `fields[0]` with argv `fields`.
    fn command(fields: &[&str]) -> Invocation {
        Invocation {
            commands: vec![(fields[0].to_string(), lists(&[fields]).remove(0))],
            environment: Vec::new(),
        }
    }

    #[test]
    fn invocations_name_their_config_directory() {
        let path =
            |fields: &[&[&str]]| invocation_config_path(lists(fields).iter().map(Vec::as_slice));
        let d = Some("/d/config.xml".to_string());
        assert_eq!(path(&[&["/usr/bin/syncthing", "serve", "--home", "/d"]]), d);
        assert_eq!(path(&[&["syncthing", "--config=/d/"]]), d);
        assert_eq!(path(&[&["syncthing", "-C/d"]]), d);
        assert_eq!(path(&[&["syncthing", "-H=/d"]]), d);
        assert_eq!(path(&[&["syncthing", "-H", "/d"]]), d);
        assert_eq!(
            path(&[&["/usr/bin/syncthing", "serve"], &["STHOMEDIR=/e"]]),
            Some("/e/config.xml".into())
        );
        // A value is one whole argument: spaces and what follows them belong
        // to the directory.
        assert_eq!(
            path(&[&["syncthing", "--home", "/srv/My Sync", "--no-browser"]]),
            Some("/srv/My Sync/config.xml".into())
        );
        assert_eq!(
            path(&[&["syncthing", "--home=/srv/Sync -archive"]]),
            Some("/srv/Sync -archive/config.xml".into())
        );
        assert_eq!(
            path(&[&["syncthing"], &["STHOMEDIR=/srv/My Sync"]]),
            Some("/srv/My Sync/config.xml".into())
        );
        assert_eq!(
            path(&[&["syncthing", "--config="]]),
            Some("config.xml".into())
        );
        assert_eq!(path(&[&["syncthing", "--home"], &["STNORESTART=1"]]), None);
        assert_eq!(path(&[&["syncthing", "serve", "--no-browser"], &[]]), None);
    }

    #[test]
    fn bus_replies_keep_every_argument_whole() {
        let reply = concat!(
            r#"{"type":"a(sasbttttuii)","data":[["/usr/bin/syncthing","#,
            r#"["/usr/bin/syncthing","serve","--home","/srv/My Sync"],false,0,0,0,0,0,0,0]]}"#,
            "\n",
            r#"{"type":"as","data":["STHOMEDIR=/srv/My Sync"]}"#,
            "\n"
        );
        assert_eq!(
            parse_invocation(reply.as_bytes()),
            Some(Invocation {
                environment: lists(&[&["STHOMEDIR=/srv/My Sync"]]).remove(0),
                ..command(&["/usr/bin/syncthing", "serve", "--home", "/srv/My Sync"])
            })
        );
        let empty = "{\"type\":\"a(sasbttttuii)\",\"data\":[]}\n{\"type\":\"as\",\"data\":[]}\n";
        assert_eq!(
            parse_invocation(empty.as_bytes()),
            Some(Invocation::default())
        );
        for bad in [
            "",
            "{\"type\":\"a(sasbttttuii)\",\"data\":[]}\n",
            "{\"type\":\"a(sasasttttuii)\",\"data\":[]}\n{\"type\":\"as\",\"data\":[]}\n",
            "{\"type\":\"a(sasbttttuii)\",\"data\":[]}\n{\"type\":\"s\",\"data\":[]}\n",
            "{\"type\":\"a(sasbttttuii)\",\"data\":[[\"/x\",[\"/x\",1],false,0,0,0,0,0,0,0]]}\n\
             {\"type\":\"as\",\"data\":[]}\n",
            "{\"type\":\"a(sasbttttuii)\",\"data\":[[\"/x\",[\"/x\"]]]}\n{\"type\":\"as\",\"data\":[]}\n",
            "{\"type\":\"a(sasbttttuii)\",\"data\":[]}\n{\"type\":\"as\",\"data\":[]}\n{}\n",
            "{\"type\":\"a(sasbttttuii)\",\"data\":[]}\n{\"type\":\"as\",\"data\":[\"A=1\"",
        ] {
            assert_eq!(parse_invocation(bad.as_bytes()), None, "{bad}");
        }
    }

    #[test]
    fn unit_object_paths_are_escaped_as_systemd_does() {
        // As systemd 262 names these units' objects (`GetUnit`), and a
        // leading digit as `sd_bus_path_encode` escapes it.
        for (unit, element) in [
            ("syncthing.service", "syncthing_2eservice"),
            ("pipewire-pulse.service", "pipewire_2dpulse_2eservice"),
            (
                "syncthing@me:x-1_.service",
                "syncthing_40me_3ax_2d1_5f_2eservice",
            ),
            ("1st.service", "_31st_2eservice"),
        ] {
            assert_eq!(
                unit_object_path(unit),
                format!("/org/freedesktop/systemd1/unit/{element}")
            );
        }
    }

    #[test]
    fn matching_needs_a_local_target_and_the_same_config() {
        let argv = |fields: &'static [&'static str]| move || Some(command(fields));
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
        let matches = |binding: &Binding, target: &Target, fields| {
            target_matches(binding, target, argv(fields))
        };
        assert_eq!(
            matches(&binding, &target, &["syncthing", "serve"]),
            Some(true)
        );
        assert_eq!(
            matches(&binding, &target, &["syncthing", "-H", "/s/"]),
            Some(true)
        );
        assert_eq!(
            matches(&binding, &target, &["syncthing", "-H", "/t"]),
            Some(false)
        );
        let flatpak = Target {
            automatic: false,
            ..target.clone()
        };
        assert_eq!(
            matches(&binding, &flatpak, &["syncthing", "serve"]),
            Some(false)
        );
        let remote = Target {
            local: false,
            ..target.clone()
        };
        assert_eq!(
            matches(&binding, &remote, &["syncthing", "-H", "/s"]),
            Some(false)
        );
        let bound = Binding {
            config_path: "/s/config.xml".into(),
            ..binding.clone()
        };
        assert_eq!(
            matches(&bound, &target, &["syncthing", "serve"]),
            Some(false)
        );
        assert_eq!(
            matches(&bound, &target, &["syncthing", "--home=/s"]),
            Some(true)
        );
        // A wrapper's command string is not parsed: its unit matches nothing,
        // whichever directory the string names. syncthing itself without
        // options runs the default configuration.
        for wrapped in [
            &["/bin/sh", "-c", "exec syncthing serve --home /t"][..],
            &["/bin/sh", "-c", "exec syncthing serve --home /s"],
            &[
                "/usr/bin/env",
                "STHOMEDIR=/s",
                "/usr/bin/syncthing",
                "serve",
            ],
        ] {
            assert_eq!(
                matches(&binding, &target, wrapped),
                Some(false),
                "{wrapped:?}"
            );
        }
        assert_eq!(
            matches(&binding, &target, &["/usr/bin/syncthing", "serve"]),
            Some(true)
        );
        // A unit without a command runs no Syncthing.
        assert_eq!(
            target_matches(&binding, &target, || Some(Invocation::default())),
            Some(false)
        );
        // An argument with a `$` is known only when systemd starts the unit:
        // `$ARGS` and `${ARGS}` from its environment, and `$$` as `$` or, with
        // the `:` prefix, as `$$`. Its unit matches nothing.
        let dollars = Target {
            config_path: "/s$$/config.xml".into(),
            ..target.clone()
        };
        for variable in [
            &["/usr/bin/syncthing", "$ARGS"][..],
            &["/usr/bin/syncthing", "${ARGS}"],
            &["/usr/bin/syncthing", "serve", "--home", "/s$$"],
        ] {
            assert_eq!(
                matches(&binding, &dollars, variable),
                Some(false),
                "{variable:?}"
            );
        }
    }
}
