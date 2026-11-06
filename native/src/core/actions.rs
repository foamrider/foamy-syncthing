//! The actions the panel uses (Syncshell `internal/session/actions.go`,
//! `device_actions.go` and `lifecycle.go`): serialized and validated like
//! Syncshell's. This is the argument shape and dispatch only; every action
//! answers `unsupported_action` until its port lands.

use serde::Deserialize;

use super::session::Session;
use super::types::ActionResult;

/// The single action input shape (strict: unknown fields are invalid).
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Arguments {
    #[serde(rename = "folderId", default)]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(rename = "createDirectory", default)]
    pub create_directory: Option<bool>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(rename = "deviceIds", default)]
    pub device_ids: Option<Vec<String>>,
    #[serde(rename = "folderIds", default)]
    pub folder_ids: Option<Vec<String>>,
    #[serde(rename = "pendingDeviceId", default)]
    pub pending_device_id: Option<String>,
    #[serde(rename = "deviceId", default)]
    pub device_id: Option<String>,
    #[serde(rename = "deviceName", default)]
    pub device_name: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
}

/// Go's zero values: `null` and absent are the same.
struct Args<'a> {
    folder_id: &'a str,
    path: &'a str,
    create_directory: bool,
    label: &'a str,
    device_ids: &'a [String],
    folder_ids: &'a [String],
    pending_device_id: &'a str,
    device_id: &'a str,
    device_name: &'a str,
    theme: &'a str,
}

impl Arguments {
    fn view(&self) -> Args<'_> {
        Args {
            folder_id: self.folder_id.as_deref().unwrap_or(""),
            path: self.path.as_deref().unwrap_or(""),
            create_directory: self.create_directory.unwrap_or(false),
            label: self.label.as_deref().unwrap_or(""),
            device_ids: self.device_ids.as_deref().unwrap_or(&[]),
            folder_ids: self.folder_ids.as_deref().unwrap_or(&[]),
            pending_device_id: self.pending_device_id.as_deref().unwrap_or(""),
            device_id: self.device_id.as_deref().unwrap_or(""),
            device_name: self.device_name.as_deref().unwrap_or(""),
            theme: self.theme.as_deref().unwrap_or(""),
        }
    }
}

fn rejected(code: &str, message: &str) -> ActionResult {
    ActionResult::rejected(code, message)
}

fn validate(action: &str, a: &Args<'_>) -> Option<ActionResult> {
    if a.create_directory && action != "folder.add-existing" {
        return Some(rejected(
            "invalid_action",
            "directory creation is only valid when adding a folder",
        ));
    }
    let others_empty = a.path.is_empty()
        && a.label.is_empty()
        && a.device_ids.is_empty()
        && a.folder_ids.is_empty()
        && a.pending_device_id.is_empty()
        && a.device_id.is_empty()
        && a.device_name.is_empty()
        && a.theme.is_empty();
    let folder_only = !a.folder_id.is_empty() && others_empty;
    let empty = a.folder_id.is_empty() && others_empty;
    let valid = match action {
        "folder.pause" | "folder.resume" | "folder.forget" => folder_only,
        "folder.add-existing" => {
            a.theme.is_empty()
                && a.folder_ids.is_empty()
                && a.device_id.is_empty()
                && a.device_name.is_empty()
        }
        _ => empty,
    };
    (!valid).then(|| rejected("invalid_action", "action arguments are invalid"))
}

impl Session {
    /// Serializes, validates and runs one action.
    pub fn act(&self, action: &str, arguments: &Arguments) -> ActionResult {
        let _serialized = self.action.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(rejection) = validate(action, &arguments.view()) {
            return rejection;
        }
        rejected("unsupported_action", "action is not supported")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn args(json: Value) -> Arguments {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn argument_shapes_are_per_action() {
        let check = |action: &str, json: Value| {
            validate(action, &args(json).view()).map(|r| r.error.unwrap().code)
        };
        assert_eq!(
            check("folder.pause", serde_json::json!({"folderId": "a"})),
            None
        );
        assert_eq!(
            check(
                "folder.pause",
                serde_json::json!({"folderId": "a", "label": "x"})
            )
            .as_deref(),
            Some("invalid_action")
        );
        assert_eq!(
            check("folder.pause", serde_json::json!({})).as_deref(),
            Some("invalid_action")
        );
        assert_eq!(
            check(
                "folder.suggest-id",
                serde_json::json!({"folderId": null, "deviceIds": []})
            ),
            None
        );
        assert_eq!(
            check("lifecycle.stop", serde_json::json!({"theme": "x"})).as_deref(),
            Some("invalid_action")
        );
        assert_eq!(
            check(
                "folder.add-existing",
                serde_json::json!({"folderId": "a", "path": "/x", "deviceIds": ["D"]})
            ),
            None
        );
        assert_eq!(
            check("folder.add-existing", serde_json::json!({"deviceId": "D"})).as_deref(),
            Some("invalid_action")
        );
        assert_eq!(
            check(
                "folder.forget",
                serde_json::json!({"folderId": "a", "createDirectory": true})
            )
            .as_deref(),
            Some("invalid_action")
        );
        // Not ported: like any unknown action.
        assert_eq!(check("folder.rescan", serde_json::json!({})), None);
        assert_eq!(
            check("folder.rescan", serde_json::json!({"folderId": "a"})).as_deref(),
            Some("invalid_action")
        );
        assert!(serde_json::from_value::<Arguments>(serde_json::json!({"other": 1})).is_err());
    }
}
