//! The bounded rotation of current file operations (Syncshell
//! `internal/session/activity.go`): at most 12 records, each held for 10 s
//! after its last event, the current one advancing every 2.5 s.

use std::collections::BTreeMap;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use super::client::{Code, Event, FileEntry};
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
    /// When the record was last stored, in store order.
    stored: u64,
}

#[derive(Default)]
pub struct Activities {
    /// `folder\0path\0action` → record (sorted, as Syncshell sorts keys).
    records: BTreeMap<String, Record>,
    index: usize,
    stores: u64,
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

/// The files of an update that are looked up.
fn local_index_files(data: &LocalIndex) -> impl Iterator<Item = &str> {
    data.filenames
        .iter()
        .take(MAX_LOCAL_INDEX_FILES)
        .map(String::as_str)
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

/// What an event batch keeps of one file's lookup. A reply's `type` can be
/// any JSON value of up to a whole reply; only its meaning is kept, so the
/// batch holds two flags per file whatever the replies' size.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Lookup {
    directory: bool,
    deleted: bool,
}

impl From<&FileEntry> for Lookup {
    fn from(entry: &FileEntry) -> Self {
        Self {
            directory: is_directory(&entry.kind),
            deleted: entry.deleted,
        }
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
            stored: self.stores,
        };
        self.stores += 1;
        let exists = self.records.contains_key(&key);
        let before = self.records.len();
        self.records.retain(|current, r| {
            current == &key || r.activity.folder_id != folder_id || r.activity.path != path
        });
        let replaced = self.records.len() != before;
        self.records.insert(key, record);
        // Over the cap the record stored longest ago goes, never the one just
        // stored (Syncshell drops the key that sorts first, which can be it).
        if self.records.len() > MAX_ACTIVITY
            && let Some(oldest) = self
                .records
                .iter()
                .min_by_key(|(_, r)| r.stored)
                .map(|(key, _)| key.clone())
        {
            self.records.remove(&oldest);
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
        let mut answered = true;
        // The files of every `LocalIndexUpdated` event are looked up first, in
        // one set of rounds, so a poll of many events waits 150 ms in all for
        // deletions to land, not 150 ms per event.
        let updates: Vec<Option<LocalIndex>> = events
            .iter()
            .map(|event| {
                (event.kind == "LocalIndexUpdated")
                    .then(|| data::<LocalIndex>(event))
                    .flatten()
                    .filter(|data| !data.folder.is_empty())
            })
            .collect();
        let lookups: Vec<(&str, &str)> = updates
            .iter()
            .flatten()
            .flat_map(|data| local_index_files(data).map(move |name| (data.folder.as_str(), name)))
            .collect();
        let mut infos = self.current_file_infos(&lookups, &mut answered).into_iter();
        for (event, update) in events.iter().zip(&updates) {
            changed = match event.kind.as_str() {
                "RemoteDownloadProgress" => activities.remote_progress(event, now),
                "DownloadProgress" => activities.download_progress(event, now),
                "LocalIndexUpdated" => update.as_ref().is_some_and(|data| {
                    self.local_index(&mut activities, data, &mut infos, now, &mut answered)
                }),
                "ItemStarted" => activities.item_started(event, now),
                "ItemFinished" => activities.item_finished(event),
                "DeviceDisconnected" => activities.device_disconnected(event),
                _ => false,
            } || changed;
        }
        changed
    }

    /// Applies one update with its files' lookups, taken from `infos` in the
    /// order of `local_index_files`. `answered` turns false once a request of
    /// this batch got no usable answer; no further rescan is sent then.
    fn local_index(
        &self,
        activities: &mut Activities,
        data: &LocalIndex,
        infos: &mut impl Iterator<Item = (Option<Lookup>, bool)>,
        now: Instant,
        answered: &mut bool,
    ) -> bool {
        let mut changed = false;
        for (name, (lookup, failed)) in local_index_files(data).zip(infos) {
            let Some(entry) = lookup.filter(|_| !failed) else {
                changed = activities.store(&data.folder, name, "syncing", "", now) || changed;
                continue;
            };
            if entry.directory && !entry.deleted {
                if *answered
                    && let Err(error) = self.client.rescan_subdirectory(&data.folder, name)
                    && error.code != Code::Http
                {
                    *answered = false;
                }
                continue;
            }
            let action = if entry.deleted { "removing" } else { "syncing" };
            changed = activities.store(&data.folder, name, action, "", now) || changed;
        }
        changed
    }

    /// Up to three reads of each `(folder, file)` 50 and 100 ms apart,
    /// settling a file early on a deletion; per file, the bool is "the latest
    /// read that was not empty failed". The files are read in rounds, so the
    /// batch waits 150 ms at most, not 150 ms per file. Only an HTTP status
    /// reply (a file not in the index yet) is worth another read: any other
    /// failure (no answer, a refused key, a reply that can't be read) would
    /// come back the same, and a request without an answer waits for its own
    /// 15 s deadline. The lookups of the batch end there; files not read yet
    /// have no entry. Of each reply only a `Lookup` is kept (the `local`
    /// entry, else the `global` one), and the reply is dropped once it is
    /// read.
    fn current_file_infos(
        &self,
        files: &[(&str, &str)],
        answered: &mut bool,
    ) -> Vec<(Option<Lookup>, bool)> {
        let mut results: Vec<(Option<Lookup>, bool)> = vec![(None, false); files.len()];
        let mut pending: Vec<usize> = (0..files.len()).collect();
        for attempt in 0..3u32 {
            if attempt > 0 {
                if pending.is_empty() || !*answered {
                    break;
                }
                thread::sleep(Duration::from_millis(50 * u64::from(attempt)));
            }
            pending.retain(|&index| {
                if !*answered {
                    return false;
                }
                let (lookup, failed) = &mut results[index];
                let (folder_id, name) = files[index];
                match self.client.file_info(folder_id, name) {
                    Ok(candidate) => match candidate.local.as_ref().or(candidate.global.as_ref()) {
                        Some(entry) => {
                            *failed = false;
                            *lookup = Some(Lookup::from(entry));
                            !entry.deleted
                        }
                        None => true,
                    },
                    Err(error) => {
                        *failed = true;
                        if error.code != Code::Http {
                            *answered = false;
                        }
                        true
                    }
                }
            });
        }
        results
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
    use crate::core::client::tests::{client_for, peer, reply};
    use crate::core::session::Operational;
    use crate::core::systemd::{Binding, Target};
    use std::sync::{Arc, Mutex};

    fn event(kind: &str, data: Value) -> Event {
        Event {
            id: 1,
            kind: kind.into(),
            data: Some(data),
        }
    }

    /// A session against a loopback Syncthing that answers each request with
    /// `answer(target, how often this target was asked before)`; the second
    /// value is every request line, in order.
    fn syncthing(
        answer: impl Fn(&str, usize) -> String + Send + 'static,
    ) -> (Session, Arc<Mutex<Vec<String>>>) {
        let asked = Mutex::new(BTreeMap::<String, usize>::new());
        let (port, heads) = peer(move |_, head| {
            let target = head.split(' ').nth(1).unwrap_or("").to_string();
            let mut asked = asked.lock().unwrap();
            let count = asked.entry(target.clone()).or_default();
            *count += 1;
            answer(&target, *count - 1)
        });
        let session = Session::new(
            client_for(port),
            Target::default(),
            String::new(),
            Binding::default(),
            String::new(),
            Operational {
                probe_interval: Duration::from_secs(15),
                refresh_interval: Duration::from_secs(60),
            },
        );
        (session, heads)
    }

    fn file(deleted: bool) -> String {
        reply(
            "200 OK",
            "",
            &format!(r#"{{"local":{{"type":"FILE_INFO_TYPE_FILE","deleted":{deleted}}}}}"#),
        )
    }

    fn local_index(names: &[&str]) -> Event {
        event(
            "LocalIndexUpdated",
            serde_json::json!({"folder": "f", "filenames": names}),
        )
    }

    fn requests(heads: &Mutex<Vec<String>>) -> Vec<String> {
        heads
            .lock()
            .unwrap()
            .iter()
            .map(|head| head.lines().next().unwrap_or("").to_string())
            .collect()
    }

    fn actions(session: &Session, now: Instant) -> Vec<(String, String)> {
        session
            .activity
            .lock()
            .unwrap()
            .rotation(false, now)
            .files
            .into_iter()
            .map(|f| (f.path, f.action))
            .collect()
    }

    #[test]
    fn a_local_index_update_waits_for_deletions_once_not_per_file() {
        // "z-gone" is deleted by the time of its second read.
        let (session, heads) =
            syncthing(|target, count| file(target.ends_with("file=z-gone") && count > 0));
        let names: Vec<String> = (0..15).map(|i| format!("k{i:02}")).collect();
        let mut all: Vec<&str> = names.iter().map(String::as_str).collect();
        all.push("z-gone");
        let now = Instant::now();
        let started = Instant::now();
        assert!(session.process_activity_events(&[local_index(&all)], now));
        // Each file waits 50 and 100 ms for a deletion to land: 150 ms for
        // the whole event, where 150 ms per file came to 2.4 s.
        let took = started.elapsed();
        assert!(took < Duration::from_secs(2), "{took:?}");
        let requests = requests(&heads);
        assert_eq!(requests.len(), 15 * 3 + 2, "{requests:?}");
        let actions = actions(&session, now);
        assert_eq!(actions.len(), 12);
        assert!(
            actions.contains(&("z-gone".into(), "removing".into())),
            "{actions:?}"
        );
    }

    #[test]
    fn a_batch_of_local_index_updates_waits_for_deletions_once() {
        let (session, heads) = syncthing(|_, _| file(false));
        let names: Vec<String> = (0..20).map(|i| format!("k{i:02}")).collect();
        let events: Vec<Event> = names.iter().map(|name| local_index(&[name])).collect();
        let now = Instant::now();
        let started = Instant::now();
        assert!(session.process_activity_events(&events, now));
        // The batch waits 150 ms in all, where 150 ms per event came to 3 s.
        let took = started.elapsed();
        assert!(took < Duration::from_secs(2), "{took:?}");
        assert_eq!(requests(&heads).len(), 20 * 3);
        assert_eq!(actions(&session, now).len(), 12);
    }

    #[test]
    fn a_lookup_without_an_answer_ends_the_lookups_of_the_batch() {
        // Syncthing drops every lookup: the first one ends the batch, where
        // each of the 16 + 2 files was asked three times, every request
        // waiting for its own deadline against a Syncthing that hangs.
        let (session, heads) = syncthing(|_, _| String::new());
        let names: Vec<String> = (0..16).map(|i| format!("k{i:02}")).collect();
        let first: Vec<&str> = names.iter().map(String::as_str).collect();
        let now = Instant::now();
        assert!(
            session.process_activity_events(&[local_index(&first), local_index(&["x", "y"])], now)
        );
        assert_eq!(requests(&heads).len(), 1);
        // A file that could not be looked up shows as syncing.
        let actions = actions(&session, now);
        assert_eq!(actions.len(), 12);
        assert!(
            actions.iter().all(|(_, action)| action == "syncing"),
            "{actions:?}"
        );

        // A rescan without an answer ends the rescans of the batch too. The
        // lookups of the whole batch come first.
        let (session, heads) = syncthing(|target, _| {
            if target.starts_with("/rest/db/scan") {
                String::new()
            } else {
                reply(
                    "200 OK",
                    "",
                    r#"{"local":{"type":"FILE_INFO_TYPE_DIRECTORY","deleted":false}}"#,
                )
            }
        });
        session.process_activity_events(&[local_index(&["photos"]), local_index(&["x"])], now);
        let requests = requests(&heads);
        assert_eq!(requests.len(), 2 * 3 + 1, "{requests:?}");
        assert!(
            requests[6].starts_with("POST /rest/db/scan?folder=f&sub=photos "),
            "{requests:?}"
        );
    }

    #[test]
    fn the_batch_keeps_two_flags_of_each_lookup_not_the_reply() {
        // A large `type` value is read for its meaning only.
        let padding = "x".repeat(64 << 10);
        let (session, _) = syncthing(move |target, _| {
            let kind = if target.ends_with("file=photos") {
                Value::from(format!("{padding}_DIRECTORY"))
            } else {
                Value::from(vec![padding.clone()])
            };
            let body = serde_json::json!({"global": {"type": kind, "deleted": false}});
            reply("200 OK", "", &body.to_string())
        });
        let mut answered = true;
        let kept = session.current_file_infos(&[("f", "photos"), ("f", "notes")], &mut answered);
        let lookup = |directory| Lookup {
            directory,
            deleted: false,
        };
        assert_eq!(
            kept,
            [(Some(lookup(true)), false), (Some(lookup(false)), false)]
        );
        assert!(answered);
    }

    #[test]
    fn a_file_not_in_the_index_yet_is_read_again() {
        // An HTTP status reply is an answer: the file is read again and the
        // other files are still looked up.
        let (session, heads) = syncthing(|target, count| {
            if target.ends_with("file=new") && count < 2 {
                reply("404 Not Found", "", "")
            } else {
                file(true)
            }
        });
        let now = Instant::now();
        assert!(session.process_activity_events(&[local_index(&["new", "old"])], now));
        assert_eq!(requests(&heads).len(), 3 + 1);
        assert_eq!(
            actions(&session, now),
            [
                ("new".to_string(), "removing".to_string()),
                ("old".to_string(), "removing".to_string())
            ]
        );
    }

    #[test]
    fn the_cap_drops_the_oldest_record_never_the_one_just_stored() {
        let now = Instant::now();
        let mut a = Activities::default();
        for index in 1..=12 {
            assert!(a.store("f", &format!("b{index:02}"), "syncing", "", now));
        }
        // "a" sorts before every other key and is the newest record.
        assert!(a.store("f", "a", "syncing", "", now));
        let paths = |a: &mut Activities| -> Vec<String> {
            a.rotation(false, now)
                .files
                .into_iter()
                .map(|f| f.path)
                .collect()
        };
        let shown = paths(&mut a);
        assert_eq!(shown.len(), 12);
        assert_eq!(shown[0], "a");
        assert!(!shown.contains(&"b01".to_string()), "{shown:?}");
        // Storing a record again makes it new: b03 goes before b02.
        assert!(!a.store("f", "b02", "syncing", "", now));
        assert!(a.store("f", "c", "syncing", "", now));
        let shown = paths(&mut a);
        assert!(shown.contains(&"b02".to_string()), "{shown:?}");
        assert!(!shown.contains(&"b03".to_string()), "{shown:?}");
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
