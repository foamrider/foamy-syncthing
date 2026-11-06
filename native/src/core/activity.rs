//! The bounded rotation of current file operations (Syncshell
//! `internal/session/activity.go`): at most 12 records, each held for 10 s
//! after its last event, the current one advancing every 2.5 s.

use std::collections::BTreeMap;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use super::client::{Event, FileEntry, FileInfo};
use super::gocompat::base;
use super::session::Session;
use super::types::{
    Activity, ActivityState, Published, bounded_identifier, bounded_label, bounded_path,
};

const ACTIVITY_HOLD: Duration = Duration::from_secs(10);
pub const ACTIVITY_CYCLE: Duration = Duration::from_millis(2500);
const MAX_ACTIVITY: usize = 12;
const MAX_LOCAL_INDEX_FILES: usize = 16;

#[derive(Clone, Debug)]
struct Record {
    activity: Activity,
    device_id: String,
    expires: Instant,
}

#[derive(Default)]
pub struct Activities {
    /// `folder\0path\0action` → record (sorted, as Syncshell sorts keys).
    records: BTreeMap<String, Record>,
    index: usize,
}

/// An event's data decoded like Go's `json.Unmarshal` into a struct: absent
/// data fails, `null` is the zero value.
fn data<T: for<'de> Deserialize<'de> + Default>(event: &Event) -> Option<T> {
    match &event.data {
        None => None,
        Some(Value::Null) => Some(T::default()),
        Some(value) => T::deserialize(value).ok(),
    }
}

fn nullable<'de, D: serde::Deserializer<'de>, T: Default + Deserialize<'de>>(
    d: D,
) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Deserialize, Default)]
struct RemoteProgress {
    #[serde(default, deserialize_with = "nullable")]
    folder: String,
    #[serde(default, deserialize_with = "nullable")]
    device: String,
    #[serde(default, deserialize_with = "nullable")]
    state: BTreeMap<String, Value>,
}

#[derive(Deserialize, Default)]
struct LocalIndex {
    #[serde(default, deserialize_with = "nullable")]
    folder: String,
    #[serde(default, deserialize_with = "nullable")]
    filenames: Vec<String>,
}

#[derive(Deserialize, Default)]
struct Item {
    #[serde(default, deserialize_with = "nullable")]
    folder: String,
    #[serde(default, deserialize_with = "nullable")]
    item: String,
    #[serde(default, deserialize_with = "nullable")]
    action: String,
}

#[derive(Deserialize, Default)]
struct DeviceEvent {
    #[serde(default, deserialize_with = "nullable")]
    id: String,
}

/// Syncshell `fileType`: a string naming a directory, or the number 1.
fn is_directory(kind: &Value) -> bool {
    match kind {
        Value::String(text) => text.to_lowercase().contains("directory"),
        Value::Number(number) => number.as_f64() == Some(1.0),
        _ => false,
    }
}

impl Activities {
    fn store(
        &mut self,
        folder_id: &str,
        path: &str,
        action: &str,
        device_id: &str,
        now: Instant,
    ) -> bool {
        let (folder_id, path) = (bounded_identifier(folder_id), bounded_path(path));
        if folder_id.is_empty() || path.is_empty() {
            return false;
        }
        let key = format!("{folder_id}\0{path}\0{action}");
        let name = base(&path);
        let detail = match action {
            "upload" => format!("Upload {name}"),
            "removing" => format!("Removing {name}"),
            _ => name,
        };
        let record = Record {
            activity: Activity {
                folder_id: folder_id.clone(),
                path: path.clone(),
                action: action.into(),
                detail: bounded_label(&detail),
            },
            device_id: bounded_identifier(device_id),
            expires: now + ACTIVITY_HOLD,
        };
        let exists = self.records.contains_key(&key);
        let before = self.records.len();
        self.records.retain(|current, r| {
            current == &key || r.activity.folder_id != folder_id || r.activity.path != path
        });
        let replaced = self.records.len() != before;
        self.records.insert(key, record);
        if self.records.len() > MAX_ACTIVITY
            && let Some(first) = self.records.keys().next().cloned()
        {
            self.records.remove(&first);
        }
        !exists || replaced
    }

    fn remove_where(&mut self, predicate: impl Fn(&Record) -> bool) -> bool {
        let before = self.records.len();
        self.records.retain(|_, record| !predicate(record));
        self.records.len() != before
    }

    fn download_progress(&mut self, event: &Event, now: Instant) -> bool {
        let Some(data) = data::<BTreeMap<String, BTreeMap<String, Value>>>(event) else {
            return false;
        };
        let mut present = Vec::new();
        let mut changed = false;
        for (folder_id, files) in &data {
            for name in files.keys() {
                present.push(format!(
                    "{}\0{}\0syncing",
                    bounded_identifier(folder_id),
                    bounded_path(name)
                ));
                changed = self.store(folder_id, name, "syncing", "", now) || changed;
            }
        }
        let before = self.records.len();
        self.records
            .retain(|key, record| record.activity.action != "syncing" || present.contains(key));
        changed || self.records.len() != before
    }

    fn remote_progress(&mut self, event: &Event, now: Instant) -> bool {
        let Some(data) = data::<RemoteProgress>(event) else {
            return false;
        };
        if data.folder.is_empty() {
            return false;
        }
        if data.state.is_empty() {
            return self.remove_where(|r| {
                r.activity.folder_id == data.folder
                    && r.device_id == data.device
                    && r.activity.action == "upload"
            });
        }
        let mut changed = false;
        for name in data.state.keys() {
            changed = self.store(&data.folder, name, "upload", &data.device, now) || changed;
        }
        changed
    }

    fn item_started(&mut self, event: &Event, now: Instant) -> bool {
        let Some(data) = data::<Item>(event) else {
            return false;
        };
        if data.folder.is_empty() || data.item.is_empty() {
            return false;
        }
        let action = if data.action.to_lowercase().contains("delete") {
            "removing"
        } else {
            "syncing"
        };
        self.store(&data.folder, &data.item, action, "", now)
    }

    fn item_finished(&mut self, event: &Event) -> bool {
        let Some(data) = data::<Item>(event) else {
            return false;
        };
        self.remove_where(|r| r.activity.folder_id == data.folder && r.activity.path == data.item)
    }

    fn device_disconnected(&mut self, event: &Event) -> bool {
        let Some(data) = data::<DeviceEvent>(event) else {
            return false;
        };
        if data.id.is_empty() {
            return false;
        }
        self.remove_where(|r| r.device_id == data.id)
    }

    /// Drops expired records, advances (or clamps) the current index, and
    /// returns the public rotation.
    fn rotation(&mut self, advance: bool, now: Instant) -> ActivityState {
        self.records.retain(|_, record| record.expires > now);
        let count = self.records.len();
        if count == 0 {
            self.index = 0;
        } else if advance {
            self.index = (self.index + 1) % count;
        } else if self.index >= count {
            self.index = 0;
        }
        let files: Vec<Activity> = self.records.values().map(|r| r.activity.clone()).collect();
        let current = files.get(self.index).cloned();
        ActivityState { files, current }
    }
}

impl Session {
    /// Applies activity events; true when the rotation changed.
    pub fn process_activity_events(&self, events: &[Event], now: Instant) -> bool {
        let mut activities = self.activity.lock().unwrap_or_else(|e| e.into_inner());
        let mut changed = false;
        for event in events {
            changed = match event.kind.as_str() {
                "RemoteDownloadProgress" => activities.remote_progress(event, now),
                "DownloadProgress" => activities.download_progress(event, now),
                "LocalIndexUpdated" => self.local_index(&mut activities, event, now),
                "ItemStarted" => activities.item_started(event, now),
                "ItemFinished" => activities.item_finished(event),
                "DeviceDisconnected" => activities.device_disconnected(event),
                _ => false,
            } || changed;
        }
        changed
    }

    fn local_index(&self, activities: &mut Activities, event: &Event, now: Instant) -> bool {
        let Some(data) = data::<LocalIndex>(event) else {
            return false;
        };
        if data.folder.is_empty() {
            return false;
        }
        let mut changed = false;
        for name in data.filenames.iter().take(MAX_LOCAL_INDEX_FILES) {
            let (info, failed) = self.current_file_info(&data.folder, name);
            let entry: Option<FileEntry> = info.local.or(info.global);
            let Some(entry) = entry.filter(|_| !failed) else {
                changed = activities.store(&data.folder, name, "syncing", "", now) || changed;
                continue;
            };
            if is_directory(&entry.kind) && !entry.deleted {
                let _ = self.client.rescan_subdirectory(&data.folder, name);
                continue;
            }
            let action = if entry.deleted { "removing" } else { "syncing" };
            changed = activities.store(&data.folder, name, action, "", now) || changed;
        }
        changed
    }

    /// Up to three reads 50 and 100 ms apart, settling early on a deletion;
    /// the bool is "the last attempt failed and no earlier one succeeded".
    fn current_file_info(&self, folder_id: &str, name: &str) -> (FileInfo, bool) {
        let mut info = FileInfo::default();
        let mut failed = false;
        for attempt in 0..3u32 {
            match self.client.file_info(folder_id, name) {
                Ok(candidate) if candidate.local.is_some() || candidate.global.is_some() => {
                    failed = false;
                    let deleted = candidate
                        .local
                        .as_ref()
                        .or(candidate.global.as_ref())
                        .is_some_and(|e| e.deleted);
                    info = candidate;
                    if deleted {
                        return (info, false);
                    }
                }
                Ok(_) => {}
                Err(_) => failed = true,
            }
            if attempt < 2 {
                thread::sleep(Duration::from_millis(50 * u64::from(attempt + 1)));
            }
        }
        (info, failed)
    }

    /// Publishes the rotation (Syncshell `publishActivity`).
    pub fn publish_activity(&self, advance: bool, now: Instant) -> Published {
        let rotation = self
            .activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .rotation(advance, now);
        self.set_activity(rotation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, data: Value) -> Event {
        Event {
            id: 1,
            kind: kind.into(),
            data: Some(data),
        }
    }

    #[test]
    fn records_rotate_replace_and_expire() {
        let now = Instant::now();
        let mut a = Activities::default();
        assert!(a.item_started(
            &event(
                "ItemStarted",
                serde_json::json!({"folder":"f","item":"d/x.txt","action":"update"})
            ),
            now
        ));
        assert!(!a.item_started(
            &event(
                "ItemStarted",
                serde_json::json!({"folder":"f","item":"d/x.txt","action":"update"})
            ),
            now
        ));
        // The same path with another action replaces the record.
        assert!(a.item_started(
            &event(
                "ItemStarted",
                serde_json::json!({"folder":"f","item":"d/x.txt","action":"delete"})
            ),
            now
        ));
        let rotation = a.rotation(false, now);
        assert_eq!(rotation.files.len(), 1);
        assert_eq!(rotation.files[0].detail, "Removing x.txt");
        assert!(a.remote_progress(
            &event(
                "RemoteDownloadProgress",
                serde_json::json!({"folder":"f","device":"D","state":{"u":1}})
            ),
            now
        ));
        assert_eq!(
            a.rotation(false, now)
                .files
                .iter()
                .find(|f| f.path == "u")
                .unwrap()
                .detail,
            "Upload u"
        );
        assert!(a.device_disconnected(&event("DeviceDisconnected", serde_json::json!({"id":"D"}))));
        assert!(a.item_finished(&event(
            "ItemFinished",
            serde_json::json!({"folder":"f","item":"d/x.txt"})
        )));
        assert!(a.rotation(false, now).files.is_empty());
        for index in 0..14 {
            a.store("f", &format!("{index:02}"), "syncing", "", now);
        }
        let rotation = a.rotation(true, now);
        assert_eq!(rotation.files.len(), 12);
        assert_eq!(rotation.current.unwrap().path, "03");
        assert!(a.rotation(false, now + ACTIVITY_HOLD).files.is_empty());
    }

    #[test]
    fn download_progress_replaces_syncing_records() {
        let now = Instant::now();
        let mut a = Activities::default();
        assert!(a.download_progress(
            &event(
                "DownloadProgress",
                serde_json::json!({"f": {"a": {}, "b": {}}})
            ),
            now
        ));
        assert!(a.download_progress(
            &event("DownloadProgress", serde_json::json!({"f": {"b": {}}})),
            now
        ));
        let paths: Vec<String> = a
            .rotation(false, now)
            .files
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(paths, ["b"]);
        assert!(a.download_progress(&event("DownloadProgress", Value::Null), now));
        assert!(!a.download_progress(&event("DownloadProgress", serde_json::json!([1])), now));
        assert!(!a.item_started(
            &Event {
                id: 1,
                kind: "ItemStarted".into(),
                data: None
            },
            now
        ));
    }

    #[test]
    fn directories_are_recognised_by_name_or_number() {
        assert!(is_directory(&Value::from("FILE_INFO_TYPE_DIRECTORY")));
        assert!(is_directory(&Value::from(1)));
        assert!(!is_directory(&Value::from(0)));
        assert!(!is_directory(&Value::from("file")));
    }
}
