//! One explicitly authorized systemd user unit (Syncshell
//! `internal/systemduser`). The core runs exactly
//! `systemctl --user show <unit> --no-pager --property=LoadState
//! --property=ActiveState --property=UnitFileState --property=FragmentPath
//! --property=ExecStart --property=Environment` (3 s), with stdin `/dev/null`,
//! output captured and capped at 64 KiB and never shown, and only for a
//! unit named on the command line with `--lifecycle-authorized`.

use std::collections::BTreeMap;
use std::time::Duration;

use foamy_helper::child::Bounded;

use super::gocompat::{clean, join};
use super::types::{Lifecycle, bounded};

const MAX_SHOW_OUTPUT: usize = 64 << 10;
const SHOW_TIMEOUT: Duration = Duration::from_secs(3);

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
    if let Some(configured) = invocation_config_path(&invocation) {
        return clean(&configured) == selected;
    }
    if !binding.config_path.is_empty() {
        return false;
    }
    target.automatic
}

/// The config directory an invocation names (`--config`, `--home`, `-C`,
/// `-H`, with a space, `=` or nothing; `STCONFDIR=`/`STHOMEDIR=`), as
/// `<dir>/config.xml`.
fn invocation_config_path(invocation: &str) -> Option<String> {
    let replaced = invocation.replace([';', '"'], " ");
    let fields: Vec<&str> = replaced.split_whitespace().collect();
    for (index, field) in fields.iter().enumerate() {
        if matches!(*field, "--config" | "--home" | "-C" | "-H") && index + 1 < fields.len() {
            return Some(join(&[fields[index + 1], "config.xml"]));
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

    #[test]
    fn invocations_name_their_config_directory() {
        let path = |s: &str| invocation_config_path(s);
        assert_eq!(
            path("{ argv[]=/usr/bin/syncthing serve --home /d ; }"),
            Some("/d/config.xml".into())
        );
        assert_eq!(path("syncthing --config=/d/"), Some("/d/config.xml".into()));
        assert_eq!(path("syncthing -C/d"), Some("/d/config.xml".into()));
        assert_eq!(path("syncthing -H=/d"), Some("/d/config.xml".into()));
        assert_eq!(
            path("\"syncthing\" \"-H\" \"/d\""),
            Some("/d/config.xml".into())
        );
        assert_eq!(
            path("/usr/bin/syncthing serve STHOMEDIR=/e"),
            Some("/e/config.xml".into())
        );
        assert_eq!(path("syncthing --config="), Some("config.xml".into()));
        assert_eq!(path("syncthing --home"), None);
        assert_eq!(path("syncthing serve --no-browser"), None);
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
    }
}
