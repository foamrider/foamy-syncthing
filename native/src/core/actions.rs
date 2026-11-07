//! The actions the panel uses (Syncshell `internal/session/actions.go`,
//! `device_actions.go` and `lifecycle.go`): serialized, validated against
//! the published snapshot and Syncthing's current configuration, and
//! followed by a refresh. Folder actions never delete or rename files.
//! Every other Syncshell action (rescans, sharing and device controls, Web
//! UI integration, enable/disable) is not ported and answers
//! `unsupported_action` like an unknown name.

use std::collections::BTreeSet;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use super::client::{self, Code, FolderDevice};
use super::gocompat::{self, base, clean, eval_symlinks, is_abs, join, paths_overlap, trim_space};
use super::session::{Session, public_error};
use super::systemd;
use super::types::*;

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

fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn rejected(code: &str, message: &str) -> ActionResult {
    ActionResult::rejected(code, message)
}

fn failed(error: &client::Error) -> ActionResult {
    ActionResult::failed(public_error(error))
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
        let args = arguments.view();
        if let Some(rejection) = validate(action, &args) {
            return rejection;
        }
        match action {
            "folder.pause" => self.set_folder_paused(args.folder_id, true),
            "folder.resume" => self.set_folder_paused(args.folder_id, false),
            "folder.recheck-errors" => self.recheck_folder_errors(),
            "folder.forget" => self.forget_folder(args.folder_id),
            "folder.add-existing" => self.add_existing_folder(&args),
            "folder.suggest-id" => self.suggest_folder_id(),
            "lifecycle.start" => self.lifecycle_action("start"),
            "lifecycle.stop" => self.lifecycle_action("stop"),
            _ => rejected("unsupported_action", "action is not supported"),
        }
    }

    fn require_online(&self) -> Option<ActionResult> {
        (!self.current().state.connection.online)
            .then(|| rejected("offline", "Syncthing is not online"))
    }

    fn refresh_after_mutation(&self) -> ActionResult {
        match self.refresh().1 {
            Some(error) => failed(&error),
            None => ActionResult::ok(),
        }
    }

    /// A connection loss or timeout may have applied the change: publish
    /// what Syncthing has now, then report the failure.
    fn after_ambiguous_mutation(&self, error: &client::Error) -> ActionResult {
        if ambiguous(error) {
            let _ = self.refresh();
        }
        failed(error)
    }

    /// The folder in the snapshot and in Syncthing's configuration, which
    /// must still agree on its ID and path.
    fn verified_folder(&self, folder_id: &str) -> Result<(Folder, client::Folder), ActionResult> {
        if let Some(rejection) = self.require_online() {
            return Err(rejection);
        }
        let folder_id = trim_space(folder_id);
        let snapshot = self
            .current()
            .state
            .folders
            .into_iter()
            .find(|f| f.id == folder_id)
            .ok_or_else(|| rejected("folder_missing", "folder is no longer configured"))?;
        let current = self.client.folder(folder_id).map_err(|e| failed(&e))?;
        if current.id != snapshot.id || current.path != snapshot.path {
            return Err(rejected(
                "folder_changed",
                "folder changed before the action",
            ));
        }
        Ok((snapshot, current))
    }

    fn set_folder_paused(&self, folder_id: &str, paused: bool) -> ActionResult {
        let (snapshot, current) = match self.verified_folder(folder_id) {
            Ok(folders) => folders,
            Err(rejection) => return rejection,
        };
        if snapshot.paused != current.paused {
            return rejected("folder_changed", "folder changed before the action");
        }
        if current.paused == paused {
            return if paused {
                rejected("folder_paused", "folder is already paused")
            } else {
                rejected("folder_active", "folder is already active")
            };
        }
        // Syncshell sends the ID as given (the snapshot lookup trims it).
        if let Err(error) = self.client.set_folder_paused(folder_id, paused) {
            return self.after_ambiguous_mutation(&error);
        }
        self.refresh_after_mutation()
    }

    fn recheck_folder_errors(&self) -> ActionResult {
        if let Some(rejection) = self.require_online() {
            return rejection;
        }
        let mut first_error = None;
        for folder in self.current().state.folders {
            let status = &folder.status;
            if folder.paused
                || (status.error.is_empty() && status.pull_errors == 0 && status.errors.is_empty())
            {
                continue;
            }
            if let Err(error) = self.client.rescan(&folder.id)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        if let Some(error) = self.refresh().1 {
            return failed(&error);
        }
        match first_error {
            Some(error) => failed(&error),
            None => ActionResult::ok(),
        }
    }

    fn forget_folder(&self, folder_id: &str) -> ActionResult {
        let (snapshot, current) = match self.verified_folder(folder_id) {
            Ok(folders) => folders,
            Err(rejection) => return rejection,
        };
        if !snapshot.paused || !current.paused {
            return rejected(
                "folder_active",
                "folder must be paused before it is forgotten",
            );
        }
        if let Err(error) = self.client.delete_folder(folder_id) {
            return self.after_ambiguous_mutation(&error);
        }
        self.refresh_after_mutation()
    }

    fn add_existing_folder(&self, args: &Args<'_>) -> ActionResult {
        if let Some(rejection) = self.require_online() {
            return rejection;
        }
        let folder_id = trim_space(args.folder_id);
        if folder_id.is_empty() || folder_id.len() > 128 || !safe_name(folder_id) {
            return rejected("folder_id_invalid", "folder ID is invalid");
        }
        let (mut canonical, validation) = canonical_directory(args.path);
        let missing = validation
            .as_ref()
            .is_some_and(|v| v.error.as_ref().is_some_and(|e| e.code == "path_missing"));
        if let Some(validation) = validation
            && !(missing && args.create_directory)
        {
            return validation;
        }
        let folders = match self.client.folders() {
            Ok(folders) => folders,
            Err(error) => return failed(&error),
        };
        let status = match self.client.status() {
            Ok(status) => status,
            Err(error) => return failed(&error),
        };
        if let Some(rejection) =
            validate_unused_folder(folder_id, &canonical, &folders, &status.tilde)
        {
            return rejection;
        }
        let devices = match self.client.devices() {
            Ok(devices) => devices,
            Err(error) => return failed(&error),
        };
        let pending = match self.client.pending_folders() {
            Ok(pending) => pending,
            Err(error) => return failed(&error),
        };
        let local_id = self.current().state.identity.device_id;
        let selected = match validate_selected_devices(args, &devices, &pending, &local_id) {
            Ok(selected) => selected,
            Err(rejection) => return rejection,
        };
        let mut defaults = match self.client.default_folder() {
            Ok(defaults) => defaults,
            Err(error) => return failed(&error),
        };
        let mut label = trim_space(args.label).to_string();
        if label.is_empty() && !args.pending_device_id.is_empty() {
            label = pending
                .get(args.folder_id)
                .and_then(|p| p.offered_by.get(args.pending_device_id))
                .map(|offer| offer.label.clone())
                .unwrap_or_default();
        }
        if label.is_empty() {
            label = base(&canonical);
        }
        let mut wanted = vec![local_id];
        wanted.extend(selected);
        defaults.insert("id".into(), Value::from(folder_id));
        defaults.insert("label".into(), Value::from(bounded_label(&label)));
        defaults.insert("path".into(), Value::from(canonical.clone()));
        defaults.insert("paused".into(), Value::from(false));
        defaults.insert(
            "devices".into(),
            serde_json::to_value(new_folder_devices(&wanted)).unwrap_or_default(),
        );
        if missing {
            if let Err(error) = create_directories(&canonical) {
                return ActionResult::rejected(
                    "path_create_failed",
                    format!("cannot create folder directory: {error}"),
                );
            }
            let (again, validation) = canonical_directory(&canonical);
            if let Some(validation) = validation {
                return validation;
            }
            canonical = again;
            if let Some(rejection) =
                validate_unused_folder(folder_id, &canonical, &folders, &status.tilde)
            {
                return rejection;
            }
            defaults.insert("path".into(), Value::from(canonical));
        }
        if let Err(error) = self.client.add_folder(&defaults) {
            return self.after_ambiguous_mutation(&error);
        }
        let mut result = self.refresh_after_mutation();
        result.data = Some(serde_json::json!({ "folderId": folder_id }));
        result
    }

    fn suggest_folder_id(&self) -> ActionResult {
        if let Some(rejection) = self.require_online() {
            return rejection;
        }
        let suggestion = match self.client.random_string(10) {
            Ok(suggestion) => trim_space(&suggestion).to_lowercase(),
            Err(error) => return failed(&error),
        };
        if suggestion.len() != 10 || !safe_name(&suggestion) {
            return rejected(
                "suggestion_invalid",
                "Syncthing returned an invalid folder ID",
            );
        }
        ActionResult {
            ok: true,
            data: Some(serde_json::json!({ "folderId": suggestion })),
            error: None,
        }
    }

    /// `systemctl --user start|stop` of the bound unit, then up to 10 s of
    /// refreshes (every 200 ms) until the unit reaches the state.
    fn lifecycle_action(&self, name: &str) -> ActionResult {
        let published = self.refresh().0;
        let state = &published.state.lifecycle;
        if !state.authorized || !state.target_match {
            return rejected(
                "lifecycle_forbidden",
                "lifecycle binding does not apply to this target",
            );
        }
        if name == "start" && !state.can_start && !state.can_control {
            return rejected(
                "lifecycle_forbidden",
                "user service cannot be started for this target",
            );
        }
        if name != "start" && !state.can_control {
            return rejected(
                "lifecycle_forbidden",
                "user service cannot be controlled for this target",
            );
        }
        if lifecycle_reached(name, &published) {
            return ActionResult::ok();
        }
        if let Err(message) = systemd::apply(&self.binding.unit, name) {
            return ActionResult::rejected("lifecycle_failed", message);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let published = self.refresh().0;
            if lifecycle_reached(name, &published) {
                return ActionResult::ok();
            }
            if Instant::now() > deadline {
                return rejected(
                    "lifecycle_timeout",
                    "user service did not reach the requested state",
                );
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
}

fn lifecycle_reached(name: &str, published: &Published) -> bool {
    let state = &published.state.lifecycle;
    match name {
        "start" => state.active_state == "active",
        "stop" => !state.active,
        _ => false,
    }
}

fn ambiguous(error: &client::Error) -> bool {
    matches!(error.code, Code::Connection | Code::Timeout)
}

/// `selectFolderDevices(nil, wanted)`: trimmed, non-empty, first occurrence.
fn new_folder_devices(wanted: &[String]) -> Vec<FolderDevice> {
    let mut seen = BTreeSet::new();
    wanted
        .iter()
        .map(|id| trim_space(id))
        .filter(|id| !id.is_empty() && seen.insert(id.to_string()))
        .map(|id| FolderDevice {
            device_id: id.to_string(),
        })
        .collect()
}

/// `os.MkdirAll(path, 0o755)`.
fn create_directories(path: &str) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(path)
        .map_err(|error| gocompat::path_error("mkdir", path, &error))
}

/// The canonical directory (links resolved) and, when it is not usable, a
/// rejection; a missing tail is `path_missing` with the would-be path.
fn canonical_directory(path: &str) -> (String, Option<ActionResult>) {
    let path = trim_space(path);
    if !is_abs(path) {
        return (
            String::new(),
            Some(rejected("path_invalid", "folder path must be absolute")),
        );
    }
    let mut existing = clean(path);
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(&existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                let message = format!(
                    "cannot access folder path: {}",
                    gocompat::path_error("lstat", &existing, &error)
                );
                return (
                    String::new(),
                    Some(ActionResult::rejected("path_unavailable", message)),
                );
            }
        }
        let parent = gocompat::dir(&existing);
        if parent == existing {
            return (
                String::new(),
                Some(rejected(
                    "path_unavailable",
                    "cannot access the filesystem root",
                )),
            );
        }
        missing.push(base(&existing));
        existing = parent;
    }
    let mut resolved = match eval_symlinks(&existing) {
        Ok(resolved) => resolved,
        Err(error) => {
            let message = format!("cannot resolve folder path or symlink: {error}");
            return (
                String::new(),
                Some(ActionResult::rejected("path_unavailable", message)),
            );
        }
    };
    match gocompat::stat(&resolved) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return (
                String::new(),
                Some(rejected("path_invalid", "folder path is not a directory")),
            );
        }
        Err(error) => {
            return (
                String::new(),
                Some(ActionResult::rejected(
                    "path_unavailable",
                    format!("cannot access folder path: {error}"),
                )),
            );
        }
    }
    for part in missing.iter().rev() {
        resolved = join(&[&resolved, part]);
    }
    if !missing.is_empty() {
        return (
            resolved,
            Some(rejected("path_missing", "folder path does not exist")),
        );
    }
    (resolved, None)
}

fn validate_unused_folder(
    folder_id: &str,
    path: &str,
    folders: &[client::Folder],
    home: &str,
) -> Option<ActionResult> {
    for folder in folders {
        if folder.id == folder_id {
            return Some(rejected("folder_exists", "folder ID is already configured"));
        }
        let mut configured = clean(&folder.path);
        if (configured == "~" || configured.starts_with("~/")) && is_abs(home) {
            configured = join(&[home, configured.trim_start_matches('~')]);
        }
        if !is_abs(&configured) {
            return Some(rejected(
                "path_unresolved",
                "cannot check folder overlap; set existing relative folder paths to absolute paths in Syncthing",
            ));
        }
        if let Ok(resolved) = eval_symlinks(&configured) {
            configured = resolved;
        }
        if paths_overlap(path, &configured) {
            return Some(rejected(
                "path_overlap",
                "folder path overlaps an existing folder",
            ));
        }
    }
    None
}

fn validate_selected_devices(
    args: &Args<'_>,
    devices: &[client::Device],
    pending: &client::PendingFolders,
    local_id: &str,
) -> Result<Vec<String>, ActionResult> {
    let selected = validate_remote_devices(args.device_ids, devices, local_id)?;
    if !args.pending_device_id.is_empty() {
        let offer = pending
            .get(args.folder_id)
            .and_then(|p| p.offered_by.get(args.pending_device_id))
            .ok_or_else(|| {
                rejected(
                    "offer_missing",
                    "selected folder offer is no longer available",
                )
            })?;
        if offer.receive_encrypted || offer.remote_encrypted {
            return Err(rejected(
                "offer_encrypted",
                "encrypted folder offers are unsupported",
            ));
        }
        if !selected.iter().any(|id| id == args.pending_device_id) {
            return Err(rejected(
                "offer_device_missing",
                "offering device must remain selected",
            ));
        }
    }
    Ok(selected)
}

/// Configured, trusted, remote; sorted and deduplicated.
fn validate_remote_devices(
    device_ids: &[String],
    devices: &[client::Device],
    local_id: &str,
) -> Result<Vec<String>, ActionResult> {
    let mut selected = BTreeSet::new();
    for raw in device_ids {
        let id = trim_space(raw);
        // The last configured entry with an ID wins, as in Go's map.
        let device = devices.iter().rev().find(|d| d.device_id == id);
        if device.is_none_or(|d| d.untrusted) || id == local_id {
            return Err(rejected(
                "device_invalid",
                "selected remote device is unavailable or untrusted",
            ));
        }
        selected.insert(id.to_string());
    }
    Ok(selected.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn new_folders_list_each_device_once() {
        let wanted = ["L".to_string(), " P ".into(), "".into(), "L".into()];
        let ids: Vec<String> = new_folder_devices(&wanted)
            .into_iter()
            .map(|d| d.device_id)
            .collect();
        assert_eq!(ids, ["L", "P"]);
    }

    #[test]
    fn folder_ids_are_safe_names() {
        assert!(safe_name("abc-12.x_y"));
        assert!(!safe_name("a b"));
        assert!(!safe_name(""));
        assert!(!safe_name("é"));
    }

    #[test]
    fn canonical_directories_resolve_links_and_report_missing_tails() {
        let base_dir = std::env::temp_dir().join(format!("foamy-canonical-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base_dir);
        std::fs::create_dir_all(base_dir.join("real")).unwrap();
        std::os::unix::fs::symlink(base_dir.join("real"), base_dir.join("link")).unwrap();
        std::fs::write(base_dir.join("file"), b"").unwrap();
        let root = std::fs::canonicalize(&base_dir)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let (path, rejection) = canonical_directory(&format!(" {root}/link/ "));
        assert_eq!(path, format!("{root}/real"));
        assert!(rejection.is_none());
        let (path, missing) = canonical_directory(&format!("{root}/link/a/b"));
        assert_eq!(path, format!("{root}/real/a/b"));
        assert_eq!(missing.unwrap().error.unwrap().code, "path_missing");
        assert_eq!(
            canonical_directory(&format!("{root}/file"))
                .1
                .unwrap()
                .error
                .unwrap()
                .code,
            "path_invalid"
        );
        assert_eq!(
            canonical_directory("relative")
                .1
                .unwrap()
                .error
                .unwrap()
                .message,
            "folder path must be absolute"
        );
        std::fs::remove_dir_all(&base_dir).unwrap();
    }
}
