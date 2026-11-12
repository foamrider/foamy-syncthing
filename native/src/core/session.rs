//! One selected Syncthing instance and its public state (Syncshell
//! `internal/session/session.go` and `normalize.go`): refresh (hydration),
//! revisioned publishing, live configuration and lifecycle probes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use super::client::{self, Client, Code};
use super::systemd::{self, Binding};
use super::types::*;

/// Live, nonsecret host intent (`configure`).
#[derive(Clone, Debug)]
pub struct Operational {
    pub probe_interval: Duration,
    pub refresh_interval: Duration,
}

pub struct Session {
    pub client: Client,
    pub target: systemd::Target,
    pub expected_device_id: String,
    pub binding: Binding,
    executable: String,
    /// Serializes refreshes; holds Syncshell's `externalLatched`.
    refresh: Mutex<bool>,
    /// Serializes actions.
    pub action: Mutex<()>,
    state: Mutex<Published>,
    config: Mutex<Operational>,
    config_changed: AtomicBool,
    pub activity: Mutex<super::activity::Activities>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Syncshell `publicError`.
pub fn public_error(error: &client::Error) -> Error {
    Error::new(error.code.as_str(), bounded_error(&error.message))
}

impl Session {
    pub fn new(
        client: Client,
        target: systemd::Target,
        expected_device_id: String,
        binding: Binding,
        executable: String,
        config: Operational,
    ) -> Session {
        Session {
            client,
            target,
            expected_device_id,
            binding,
            executable,
            refresh: Mutex::new(false),
            action: Mutex::new(()),
            state: Mutex::new(Published::default()),
            config: Mutex::new(config),
            config_changed: AtomicBool::new(false),
            activity: Mutex::new(super::activity::Activities::default()),
        }
    }

    pub fn current(&self) -> Published {
        lock(&self.state).clone()
    }

    pub fn probe_interval(&self) -> Duration {
        lock(&self.config).probe_interval
    }

    pub fn refresh_interval(&self) -> Duration {
        lock(&self.config).refresh_interval
    }

    /// True once after each `configure`.
    pub fn take_config_changed(&self) -> bool {
        self.config_changed.swap(false, Ordering::SeqCst)
    }

    /// Rebuilds the authoritative state; the error is the refresh's own.
    pub fn refresh(&self) -> (Published, Option<client::Error>) {
        let mut latched = lock(&self.refresh);
        let (snapshot, error) = self.hydrate(&mut latched);
        (self.publish(snapshot), error)
    }

    fn hydrate(&self, latched: &mut bool) -> (Snapshot, Option<client::Error>) {
        let previous = self.current().state;
        let mut snapshot = Snapshot {
            connection: Connection {
                phase: "loading".into(),
                endpoint: self.client.endpoint().into(),
                ..Connection::default()
            },
            identity: previous.identity,
            devices: previous.devices,
            folders: previous.folders,
            pending_folders: previous.pending_folders,
            pending_devices: previous.pending_devices,
            nearby_devices: previous.nearby_devices,
            activity: previous.activity,
            web_ui: previous.web_ui,
            installation: Installation {
                executable_path: bounded_path(&self.executable),
                available: !self.executable.is_empty(),
            },
            truncation: previous.truncation,
            ..Snapshot::default()
        };
        if let Err(error) = self.client.health() {
            snapshot.connection.phase = "error".into();
            snapshot.connection.error = Some(public_error(&error));
            snapshot.lifecycle = self.failure_lifecycle(*latched);
            return (snapshot, Some(error));
        }
        snapshot.connection.healthy = true;
        let status = match self.client.status() {
            Ok(status) => status,
            Err(error) => {
                snapshot.connection.phase = "error".into();
                snapshot.connection.error = Some(public_error(&error));
                snapshot.lifecycle = external(self.failure_lifecycle(*latched));
                *latched = true;
                return (snapshot, Some(error));
            }
        };
        snapshot.connection.authorized = true;
        if !self.expected_device_id.is_empty() && self.expected_device_id != status.my_id {
            let error = client::Error::new(
                Code::Identity,
                "Syncthing device identity does not match the selected target",
            );
            snapshot.connection.phase = "error".into();
            snapshot.connection.error = Some(public_error(&error));
            snapshot.lifecycle = external(self.probe_lifecycle(false));
            *latched = true;
            return (snapshot, Some(error));
        }
        let hydrated = match self.load_authenticated(&status) {
            Ok(hydrated) => hydrated,
            Err(error) => {
                snapshot.connection.phase = "error".into();
                snapshot.connection.error = Some(public_error(&error));
                snapshot.lifecycle = self.failure_lifecycle(*latched);
                return (snapshot, Some(error));
            }
        };
        snapshot.counts = normalized_counts(
            &hydrated.devices,
            &hydrated.folders,
            &hydrated.identity.device_id,
        );
        snapshot.identity = hydrated.identity;
        snapshot.devices = hydrated.devices;
        snapshot.folders = hydrated.folders;
        snapshot.pending_folders = hydrated.pending_folders;
        snapshot.pending_devices = hydrated.pending_devices;
        snapshot.nearby_devices = hydrated.nearby_devices;
        snapshot.web_ui = hydrated.web_ui;
        snapshot.truncation = hydrated.truncation;
        snapshot.connection.phase = "ready".into();
        snapshot.connection.online = true;
        snapshot.connection.fresh = true;
        snapshot.connection.error = None;
        snapshot.lifecycle = self.probe_lifecycle(true);
        *latched = snapshot.lifecycle.classification == "external";
        (snapshot, None)
    }

    fn load_authenticated(&self, status: &client::SystemStatus) -> Result<Hydrated, client::Error> {
        let version = self.client.version()?;
        let devices = self.client.devices()?;
        let folders = self.client.folders()?;
        let connections = self.client.connections()?;
        let mut normalized_folders = Vec::with_capacity(folders.len().min(MAX_FOLDERS));
        let mut omitted_errors = 0i64;
        for folder in folders.iter().take(MAX_FOLDERS) {
            let folder_status = self.client.folder_status(&folder.id)?;
            let errors = self.client.folder_errors(&folder.id)?;
            let (normalized, omitted) = normalize_folder(folder, &folder_status, &errors);
            normalized_folders.push(normalized);
            omitted_errors += omitted;
        }
        normalized_folders.sort_by(|a, b| a.id.cmp(&b.id));
        let pending = self.client.pending_folders()?;
        let pending_devices = self.client.pending_devices()?;
        let discovery = self.client.discovery_cache()?;
        let gui = self.client.gui_config()?;
        let paths = self.client.system_paths()?;
        let mut truncation = collection_truncation(&devices, &folders, &pending);
        truncation.pending_devices =
            pending_devices.len().saturating_sub(MAX_PENDING_DEVICES) as i64;
        truncation.nearby_devices = nearby_device_ids(&discovery, &devices, &status.my_id)
            .len()
            .saturating_sub(MAX_NEARBY_DEVICES) as i64;
        truncation.folder_errors = omitted_errors;
        Ok(Hydrated {
            identity: Identity {
                device_id: bounded_identifier(&status.my_id),
                version: bounded_label(&version.version),
            },
            devices: normalize_devices(&devices, &connections),
            folders: normalized_folders,
            pending_folders: normalize_pending_folders(&pending),
            pending_devices: normalize_pending_devices(&pending_devices),
            nearby_devices: normalize_nearby_devices(&discovery, &devices, &status.my_id),
            web_ui: WebUi {
                url: web_url(self.client.endpoint()),
                theme: bounded_identifier(&gui.theme),
                gui_assets: bounded_path(&paths.gui_assets),
            },
            truncation,
        })
    }

    fn failure_lifecycle(&self, latched: bool) -> Lifecycle {
        let state = self.probe_lifecycle(false);
        if latched {
            external_keep_match(state)
        } else {
            state
        }
    }

    fn probe_lifecycle(&self, api_online: bool) -> Lifecycle {
        systemd::probe(&self.binding, &self.target, api_online)
    }

    /// The scheduled lifecycle probe between refreshes.
    pub fn refresh_lifecycle(&self) -> Published {
        let latched = lock(&self.refresh);
        let current = self.current();
        if current.revision == 0 {
            return current;
        }
        // Syncshell probed once as online and then again as offline when
        // offline; one probe with the right view gives the same state.
        let state = if current.state.connection.online {
            self.probe_lifecycle(true)
        } else {
            self.failure_lifecycle(*latched)
        };
        let mut published = lock(&self.state);
        if published.state.lifecycle != state {
            published.state.lifecycle = state;
            published.revision += 1;
        }
        drop(latched);
        published.clone()
    }

    pub fn publish(&self, snapshot: Snapshot) -> Published {
        let mut published = lock(&self.state);
        if published.revision == 0 || published.state != snapshot {
            published.revision += 1;
            published.state = snapshot;
        }
        published.clone()
    }

    /// Marks the event stream healthy or not.
    pub fn set_fresh(&self, fresh: bool) -> Published {
        let mut published = lock(&self.state);
        if published.revision != 0 && published.state.connection.fresh != fresh {
            published.state.connection.fresh = fresh;
            published.revision += 1;
        }
        published.clone()
    }

    /// Replaces the activity rotation when it changed.
    pub fn set_activity(&self, activity: ActivityState) -> Published {
        let mut published = lock(&self.state);
        if published.revision != 0 && !published.state.activity.same(&activity) {
            published.state.activity = activity;
            published.revision += 1;
        }
        published.clone()
    }

    /// Validated operational intent, in memory only.
    pub fn configure(&self, config: &OperationalConfig) -> ActionResult {
        if let Some(rejection) = validate_operational_config(config) {
            return rejection;
        }
        {
            let mut current = lock(&self.config);
            if let Some(seconds) = config.probe_interval_seconds {
                current.probe_interval = Duration::from_secs(seconds as u64);
            }
            if let Some(seconds) = config.refresh_interval_seconds {
                current.refresh_interval = Duration::from_secs(seconds as u64);
            }
        }
        self.config_changed.store(true, Ordering::SeqCst);
        ActionResult::ok()
    }
}

/// The `configure` request's `config` (strict: unknown fields are invalid).
#[derive(serde::Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct OperationalConfig {
    #[serde(rename = "probeIntervalSeconds", default)]
    pub probe_interval_seconds: Option<i64>,
    #[serde(rename = "refreshIntervalSeconds", default)]
    pub refresh_interval_seconds: Option<i64>,
}

fn validate_operational_config(config: &OperationalConfig) -> Option<ActionResult> {
    if let Some(seconds) = config.probe_interval_seconds
        && !(1..=3600).contains(&seconds)
    {
        return Some(ActionResult::rejected(
            "invalid_config",
            "probe interval must be between 1 and 3600 seconds",
        ));
    }
    if let Some(seconds) = config.refresh_interval_seconds
        && !(60..=3600).contains(&seconds)
    {
        return Some(ActionResult::rejected(
            "invalid_config",
            "refresh interval must be between 60 and 3600 seconds",
        ));
    }
    None
}

/// An instance that answered but is not ours to control.
fn external(mut state: Lifecycle) -> Lifecycle {
    state.classification = "external".into();
    state.target_match = false;
    state.can_control = false;
    state.can_start = false;
    state
}

/// Syncshell's latched classification after an API loss: external, without
/// control, but the unit's own match stays visible.
fn external_keep_match(mut state: Lifecycle) -> Lifecycle {
    state.classification = "external".into();
    state.can_control = false;
    state.can_start = false;
    state
}

struct Hydrated {
    identity: Identity,
    devices: Vec<Device>,
    folders: Vec<Folder>,
    pending_folders: BTreeMap<String, PendingFolder>,
    pending_devices: Vec<PendingDevice>,
    nearby_devices: Vec<NearbyDevice>,
    web_ui: WebUi,
    truncation: Truncation,
}

fn normalize_devices(wire: &[client::Device], connections: &client::Connections) -> Vec<Device> {
    let mut result: Vec<Device> = wire
        .iter()
        .take(MAX_DEVICES)
        .map(|device| Device {
            id: bounded_identifier(&device.device_id),
            name: bounded_label(&device.name),
            untrusted: device.untrusted,
            connected: connections
                .connections
                .get(&device.device_id)
                .is_some_and(|c| c.connected),
        })
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}

fn normalized_counts(devices: &[Device], folders: &[Folder], local_device_id: &str) -> Counts {
    let mut counts = Counts {
        folders: folders.len() as i64,
        devices: devices.len() as i64,
        ..Counts::default()
    };
    for device in devices {
        if device.id == local_device_id {
            counts.devices -= 1;
            continue;
        }
        if device.connected {
            counts.connected_devices += 1;
        }
    }
    for folder in folders {
        let status = &folder.status;
        if status.state == "error"
            || !status.error.is_empty()
            || status.pull_errors > 0
            || !status.errors.is_empty()
        {
            counts.folder_problems += 1;
        }
        if status.need_total_items > 0 {
            counts.syncing_folders += 1;
        }
    }
    counts
}

fn normalize_folder(
    folder: &client::Folder,
    status: &client::FolderStatus,
    errors: &client::FolderErrors,
) -> (Folder, i64) {
    let mut devices: Vec<FolderDevice> = folder
        .devices
        .iter()
        .take(MAX_FOLDER_DEVICES)
        .map(|d| FolderDevice {
            id: bounded_identifier(&d.device_id),
        })
        .collect();
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    let mut current = errors.errors.clone();
    current.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.error.cmp(&b.error)));
    let folder_errors = current
        .iter()
        .take(MAX_FOLDER_ERRORS)
        .map(|e| FolderError {
            path: bounded_label(&e.path),
            error: bounded_error(&e.error),
        })
        .collect();
    let omitted = (status.pull_errors.max(current.len() as i64) - MAX_FOLDER_ERRORS as i64).max(0);
    (
        Folder {
            id: bounded_identifier(&folder.id),
            label: bounded_label(&folder.label),
            path: bounded_path(&folder.path),
            paused: folder.paused,
            marker_name: bounded_identifier(&folder.marker_name),
            devices,
            status: FolderStatus {
                state: bounded_label(&status.state),
                error: bounded_error(&status.error),
                pull_errors: status.pull_errors,
                need_total_items: status.need_total_items,
                need_bytes: status.need_bytes,
                global_files: status.global_files,
                global_bytes: status.global_bytes,
                errors: folder_errors,
            },
        },
        omitted,
    )
}

fn normalize_pending_folders(pending: &client::PendingFolders) -> BTreeMap<String, PendingFolder> {
    let mut result = BTreeMap::new();
    for (folder_id, folder) in pending.iter().take(MAX_PENDING_FOLDERS) {
        let mut offers = BTreeMap::new();
        for (device_id, offer) in folder.offered_by.iter().take(MAX_PENDING_OFFERS) {
            offers.insert(
                bounded_identifier(device_id),
                FolderOffer {
                    label: bounded_label(&offer.label),
                    receive_encrypted: offer.receive_encrypted,
                    remote_encrypted: offer.remote_encrypted,
                },
            );
        }
        result.insert(
            bounded_identifier(folder_id),
            PendingFolder { offered_by: offers },
        );
    }
    result
}

fn normalize_pending_devices(
    pending: &BTreeMap<String, client::PendingDevice>,
) -> Vec<PendingDevice> {
    pending
        .iter()
        .take(MAX_PENDING_DEVICES)
        .map(|(id, device)| PendingDevice {
            id: bounded_identifier(id),
            name: bounded_label(&device.name),
            address: bounded_label(&device.address),
        })
        .collect()
}

fn nearby_device_ids<'a>(
    discovery: &'a BTreeMap<String, client::DiscoveryEntry>,
    configured: &[client::Device],
    local_id: &str,
) -> Vec<&'a String> {
    let mut ignored: BTreeSet<&str> = configured.iter().map(|d| d.device_id.as_str()).collect();
    ignored.insert(local_id);
    discovery
        .keys()
        .filter(|id| !ignored.contains(id.as_str()))
        .collect()
}

fn normalize_nearby_devices(
    discovery: &BTreeMap<String, client::DiscoveryEntry>,
    configured: &[client::Device],
    local_id: &str,
) -> Vec<NearbyDevice> {
    nearby_device_ids(discovery, configured, local_id)
        .into_iter()
        .take(MAX_NEARBY_DEVICES)
        .map(|id| NearbyDevice {
            id: bounded_identifier(id),
            addresses: discovery[id]
                .addresses
                .iter()
                .take(MAX_DEVICE_ADDRESSES)
                .map(|a| bounded_label(a))
                .collect(),
        })
        .collect()
}

fn collection_truncation(
    devices: &[client::Device],
    folders: &[client::Folder],
    pending: &client::PendingFolders,
) -> Truncation {
    let mut truncation = Truncation {
        devices: devices.len().saturating_sub(MAX_DEVICES) as i64,
        folders: folders.len().saturating_sub(MAX_FOLDERS) as i64,
        pending_folders: pending.len().saturating_sub(MAX_PENDING_FOLDERS) as i64,
        ..Truncation::default()
    };
    for folder in folders.iter().take(MAX_FOLDERS) {
        truncation.folder_devices += folder.devices.len().saturating_sub(MAX_FOLDER_DEVICES) as i64;
    }
    for folder in pending.values().take(MAX_PENDING_FOLDERS) {
        truncation.pending_offers +=
            folder.offered_by.len().saturating_sub(MAX_PENDING_OFFERS) as i64;
    }
    truncation
}

fn web_url(endpoint: &str) -> String {
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.into()
    } else {
        String::new()
    }
}
