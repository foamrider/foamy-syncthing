//! The public snapshot (Syncshell `internal/session/types.go` and
//! `systemduser.State`). Field names, order and `omitempty` rules are the
//! protocol's: `Facade.js` and `models/*.js` read them. Like the Go core's
//! published copies, an empty list is written as `null` and the pending
//! folder maps always as objects.

use std::collections::BTreeMap;

use serde::{Serialize, Serializer};

pub const MAX_PUBLIC_STRING: usize = 4096;
pub const MAX_IDENTIFIER: usize = 256;
pub const MAX_LABEL: usize = 512;
/// A Syncthing version ("v2.0.10", "v2.1.0-rc.2+12-g0123abc") is short.
pub const MAX_VERSION: usize = 40;
pub const MAX_ERROR_TEXT: usize = 1024;
pub const MAX_DEVICES: usize = 256;
pub const MAX_FOLDERS: usize = 128;
pub const MAX_FOLDER_DEVICES: usize = 64;
pub const MAX_FOLDER_ERRORS: usize = 4;
pub const MAX_PENDING_FOLDERS: usize = 32;
pub const MAX_PENDING_OFFERS: usize = 16;
pub const MAX_PENDING_DEVICES: usize = 64;
pub const MAX_NEARBY_DEVICES: usize = 64;
pub const MAX_DEVICE_ADDRESSES: usize = 8;

fn null_if_empty<S: Serializer, T: Serialize>(items: &[T], s: S) -> Result<S::Ok, S::Error> {
    if items.is_empty() {
        s.serialize_none()
    } else {
        items.serialize(s)
    }
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

/// A sanitized public failure.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: String,
    pub message: String,
}

impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Error {
        Error {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Connection {
    pub phase: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub endpoint: String,
    pub healthy: bool,
    pub authorized: bool,
    pub online: bool,
    pub fresh: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    #[serde(rename = "deviceId", skip_serializing_if = "String::is_empty")]
    pub device_id: String,
    /// This Syncthing's version (bounded_version), fetched once per connection.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub version: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub untrusted: bool,
    pub connected: bool,
    /// The peer's Syncthing version while it is connected (bounded_version).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub version: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct FolderDevice {
    pub id: String,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderStatus {
    pub state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(rename = "pullErrors")]
    pub pull_errors: i64,
    #[serde(rename = "needTotalItems")]
    pub need_total_items: i64,
    #[serde(rename = "needBytes")]
    pub need_bytes: i64,
    #[serde(rename = "globalFiles")]
    pub global_files: i64,
    #[serde(rename = "globalBytes")]
    pub global_bytes: i64,
    #[serde(serialize_with = "null_if_empty")]
    pub errors: Vec<FolderError>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct FolderError {
    pub path: String,
    pub error: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub id: String,
    pub label: String,
    pub path: String,
    pub paused: bool,
    #[serde(rename = "markerName", skip_serializing_if = "String::is_empty")]
    pub marker_name: String,
    #[serde(serialize_with = "null_if_empty")]
    pub devices: Vec<FolderDevice>,
    pub status: FolderStatus,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct FolderOffer {
    pub label: String,
    #[serde(rename = "receiveEncrypted")]
    pub receive_encrypted: bool,
    #[serde(rename = "remoteEncrypted")]
    pub remote_encrypted: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PendingFolder {
    #[serde(rename = "offeredBy")]
    pub offered_by: BTreeMap<String, FolderOffer>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingDevice {
    pub id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub address: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct NearbyDevice {
    pub id: String,
    #[serde(serialize_with = "null_if_empty")]
    pub addresses: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    #[serde(rename = "folderId")]
    pub folder_id: String,
    pub path: String,
    pub action: String,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ActivityState {
    #[serde(serialize_with = "null_if_empty")]
    pub files: Vec<Activity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<Activity>,
}

impl ActivityState {
    /// Syncshell `activityEqual`: the same files and the same current one
    /// (by folder, path and action).
    pub fn same(&self, other: &ActivityState) -> bool {
        let key = |a: &Option<Activity>| {
            a.as_ref()
                .map(|a| (a.folder_id.clone(), a.path.clone(), a.action.clone()))
        };
        key(&self.current) == key(&other.current) && self.files == other.files
    }
}

impl PartialEq for ActivityState {
    fn eq(&self, other: &Self) -> bool {
        self.files == other.files && self.current == other.current
    }
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebUi {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub theme: String,
    #[serde(rename = "guiAssets", skip_serializing_if = "String::is_empty")]
    pub gui_assets: String,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Installation {
    #[serde(rename = "executablePath", skip_serializing_if = "String::is_empty")]
    pub executable_path: String,
    pub available: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub folders: i64,
    pub devices: i64,
    #[serde(rename = "connectedDevices")]
    pub connected_devices: i64,
    #[serde(rename = "folderProblems")]
    pub folder_problems: i64,
    #[serde(rename = "syncingFolders")]
    pub syncing_folders: i64,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Truncation {
    #[serde(skip_serializing_if = "is_zero")]
    pub devices: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub folders: i64,
    #[serde(rename = "folderDevices", skip_serializing_if = "is_zero")]
    pub folder_devices: i64,
    #[serde(rename = "folderErrors", skip_serializing_if = "is_zero")]
    pub folder_errors: i64,
    #[serde(rename = "pendingFolders", skip_serializing_if = "is_zero")]
    pub pending_folders: i64,
    #[serde(rename = "pendingOffers", skip_serializing_if = "is_zero")]
    pub pending_offers: i64,
    #[serde(rename = "pendingDevices", skip_serializing_if = "is_zero")]
    pub pending_devices: i64,
    #[serde(rename = "nearbyDevices", skip_serializing_if = "is_zero")]
    pub nearby_devices: i64,
}

/// The observed systemd user-service state (Syncshell `systemduser.State`).
#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Lifecycle {
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unit: String,
    pub classification: String,
    pub available: bool,
    pub active: bool,
    #[serde(rename = "activeState", skip_serializing_if = "String::is_empty")]
    pub active_state: String,
    #[serde(rename = "unitFileState", skip_serializing_if = "String::is_empty")]
    pub unit_file_state: String,
    pub authorized: bool,
    #[serde(rename = "targetMatch")]
    pub target_match: bool,
    #[serde(rename = "canControl")]
    pub can_control: bool,
    #[serde(rename = "canStart")]
    pub can_start: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// The complete public state at one revision.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub connection: Connection,
    pub identity: Identity,
    #[serde(serialize_with = "null_if_empty")]
    pub devices: Vec<Device>,
    #[serde(serialize_with = "null_if_empty")]
    pub folders: Vec<Folder>,
    #[serde(rename = "pendingFolders")]
    pub pending_folders: BTreeMap<String, PendingFolder>,
    #[serde(rename = "pendingDevices", serialize_with = "null_if_empty")]
    pub pending_devices: Vec<PendingDevice>,
    #[serde(rename = "nearbyDevices", serialize_with = "null_if_empty")]
    pub nearby_devices: Vec<NearbyDevice>,
    pub activity: ActivityState,
    #[serde(rename = "webUi")]
    pub web_ui: WebUi,
    pub installation: Installation,
    pub counts: Counts,
    pub truncation: Truncation,
    pub lifecycle: Lifecycle,
}

/// A snapshot with its monotonic revision (0: nothing published yet).
#[derive(Clone, Debug, Default)]
pub struct Published {
    pub revision: u64,
    pub state: Snapshot,
}

/// One correlated action result.
#[derive(Clone, Debug, Default)]
pub struct ActionResult {
    pub ok: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<Error>,
}

impl ActionResult {
    pub fn ok() -> ActionResult {
        ActionResult {
            ok: true,
            ..ActionResult::default()
        }
    }

    pub fn rejected(code: &str, message: impl Into<String>) -> ActionResult {
        ActionResult {
            error: Some(Error::new(code, message)),
            ..ActionResult::default()
        }
    }

    pub fn failed(error: Error) -> ActionResult {
        ActionResult {
            error: Some(error),
            ..ActionResult::default()
        }
    }
}

/// Syncshell's `bounded*`: trimmed, then cut to a byte length.
pub fn bounded_identifier(value: &str) -> String {
    bounded(value, MAX_IDENTIFIER)
}

pub fn bounded_label(value: &str) -> String {
    bounded(value, MAX_LABEL)
}

pub fn bounded_error(value: &str) -> String {
    bounded(value, MAX_ERROR_TEXT)
}

pub fn bounded_path(value: &str) -> String {
    bounded(value, MAX_PUBLIC_STRING)
}

/// A version string, or "" when it is empty, longer than MAX_VERSION or holds
/// anything but printable ASCII without spaces. Peers report their own
/// version, so it is untrusted: rejected whole rather than cut, so a shown
/// version is always one the peer sent.
pub fn bounded_version(value: &str) -> String {
    let value = super::gocompat::trim_space(value);
    if value.len() > MAX_VERSION || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return String::new();
    }
    value.to_string()
}

pub fn bounded(value: &str, max: usize) -> String {
    super::gocompat::truncate_bytes(super::gocompat::trim_space(value), max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_snapshot_serializes_like_the_go_core() {
        let snapshot = Snapshot::default();
        assert_eq!(
            serde_json::to_string(&snapshot).unwrap(),
            concat!(
                r#"{"connection":{"phase":"","healthy":false,"authorized":false,"online":false,"fresh":false},"#,
                r#""identity":{},"devices":null,"folders":null,"pendingFolders":{},"pendingDevices":null,"#,
                r#""nearbyDevices":null,"activity":{"files":null},"webUi":{},"installation":{"available":false},"#,
                r#""counts":{"folders":0,"devices":0,"connectedDevices":0,"folderProblems":0,"syncingFolders":0},"#,
                r#""truncation":{},"lifecycle":{"kind":"","classification":"","available":false,"active":false,"#,
                r#""authorized":false,"targetMatch":false,"canControl":false,"canStart":false}}"#
            )
        );
    }

    #[test]
    fn bounds_trim_then_cut_bytes() {
        assert_eq!(bounded_label("  x  "), "x");
        assert_eq!(bounded("\u{a0}abcdef\u{2003}", 3), "abc");
        assert_eq!(bounded_identifier(&"é".repeat(200)).len(), 256);
    }

    #[test]
    fn versions_are_short_printable_ascii_or_nothing() {
        assert_eq!(bounded_version(" v2.0.10\n"), "v2.0.10");
        assert_eq!(
            bounded_version("v2.1.0-rc.2+12-g0123abc"),
            "v2.1.0-rc.2+12-g0123abc"
        );
        let longest = format!("v{}", "1".repeat(MAX_VERSION - 1));
        assert_eq!(bounded_version(&longest), longest);
        assert_eq!(bounded_version(&format!("{longest}1")), "");
        for bad in [
            "v2 0",
            "v2.0\u{1b}[31m",
            "v2.0\u{202e}",
            "v2.0\u{0}",
            "versión",
            "",
        ] {
            assert_eq!(bounded_version(bad), "", "{bad:?}");
        }
    }
}
