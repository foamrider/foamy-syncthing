//! Syncthing's REST API for one selected target (Syncshell
//! `internal/syncthing`: client.go, mutations.go, events.go, wire.go,
//! errors.go). The API key lives only in [`Client`] and goes out only as the
//! `X-API-Key` header of authenticated requests on the selected
//! connection; it is never formatted, logged or returned.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::gocompat::{path_escape, query_escape};
use super::http::{self, Transport};

const MAX_RESPONSE_BYTES: usize = 8 << 20;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const EVENT_TIMEOUT: Duration = Duration::from_secs(70);
/// Go's `CheckRedirect` refuses the third redirect.
const MAX_REDIRECTS: usize = 2;

/// Syncshell's stable failure classes (`ErrorCode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    Config,
    Credential,
    Ambiguity,
    Dns,
    Connection,
    Timeout,
    Tls,
    Http,
    Unauthorized,
    Schema,
    Identity,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Config => "config",
            Code::Credential => "credential",
            Code::Ambiguity => "ambiguity",
            Code::Dns => "dns",
            Code::Connection => "connection",
            Code::Timeout => "timeout",
            Code::Tls => "tls",
            Code::Http => "http",
            Code::Unauthorized => "unauthorized",
            Code::Schema => "schema",
            Code::Identity => "identity",
        }
    }
}

/// A sanitized failure: a class and a fixed message, never the cause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: Code,
    pub message: String,
    /// The HTTP status for [`Code::Http`].
    pub status: u16,
}

impl Error {
    pub fn new(code: Code, message: impl Into<String>) -> Error {
        Error {
            code,
            message: message.into(),
            status: 0,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// A transport failure as the panel shows it. On a CPU without the
/// instructions the TLS cryptography needs, the kit's message names them.
fn transport_error(failure: http::Failure) -> Error {
    match failure {
        http::Failure::Tls => Error::new(Code::Tls, "Syncthing TLS verification failed"),
        http::Failure::TlsUnsupported => Error::new(Code::Tls, foamy_helper::tls::UNSUPPORTED),
        http::Failure::Timeout => Error::new(Code::Timeout, "request timed out"),
        http::Failure::Dns => Error::new(Code::Dns, "target name could not be resolved"),
        http::Failure::Connection => Error::new(Code::Connection, "could not connect to Syncthing"),
    }
}

/// The API key; deliberately without `Debug` output of its value.
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: String) -> ApiKey {
        ApiKey(value)
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(redacted)")
    }
}

pub struct Client {
    transport: Transport,
    api_key: ApiKey,
    /// `http` or `https`, for same-origin redirect checks.
    scheme: &'static str,
    /// The sanitized target endpoint (`http://…`, `https://…`, `unix://…`).
    endpoint: String,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Client{{Endpoint:{:?}}}", self.endpoint)
    }
}

/// `null` (or a missing field) decodes as the zero value, as in Go.
fn nullable<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// A present value, `null` included (absent stays `None`).
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Deserialize, Default, Debug)]
pub struct SystemStatus {
    #[serde(rename = "myID", default, deserialize_with = "nullable")]
    pub my_id: String,
    #[serde(default, deserialize_with = "nullable")]
    pub tilde: String,
    /// When this Syncthing process started; a restart changes it.
    #[serde(rename = "startTime", default, deserialize_with = "nullable")]
    pub start_time: String,
}

#[derive(Deserialize, Default)]
pub struct SystemVersion {
    #[serde(default, deserialize_with = "nullable")]
    pub version: String,
}

#[derive(Deserialize, Default, Clone)]
pub struct Device {
    #[serde(rename = "deviceID", default, deserialize_with = "nullable")]
    pub device_id: String,
    #[serde(default, deserialize_with = "nullable")]
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub untrusted: bool,
}

/// One folder-sharing relationship; a missing or empty `deviceID` makes the
/// whole response invalid (Syncshell `FolderDevice.UnmarshalJSON`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderDevice {
    pub device_id: String,
}

impl<'de> Deserialize<'de> for FolderDevice {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fields = Option::<Map<String, Value>>::deserialize(d)?;
        match fields.as_ref().and_then(|f| f.get("deviceID")) {
            Some(Value::String(id)) if !id.is_empty() => Ok(FolderDevice {
                device_id: id.clone(),
            }),
            _ => Err(serde::de::Error::custom("folder device ID is invalid")),
        }
    }
}

impl Serialize for FolderDevice {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = BTreeMap::new();
        map.insert("deviceID", &self.device_id);
        map.serialize(s)
    }
}

#[derive(Deserialize, Default, Clone)]
pub struct Folder {
    #[serde(default, deserialize_with = "nullable")]
    pub id: String,
    #[serde(default, deserialize_with = "nullable")]
    pub label: String,
    #[serde(default, deserialize_with = "nullable")]
    pub path: String,
    #[serde(default, deserialize_with = "nullable")]
    pub paused: bool,
    #[serde(rename = "markerName", default, deserialize_with = "nullable")]
    pub marker_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub devices: Vec<FolderDevice>,
}

#[derive(Deserialize, Default, Clone)]
pub struct FolderStatus {
    #[serde(default, deserialize_with = "nullable")]
    pub state: String,
    #[serde(default, deserialize_with = "nullable")]
    pub error: String,
    #[serde(rename = "pullErrors", default, deserialize_with = "nullable")]
    pub pull_errors: i64,
    #[serde(rename = "needTotalItems", default, deserialize_with = "nullable")]
    pub need_total_items: i64,
    #[serde(rename = "needBytes", default, deserialize_with = "nullable")]
    pub need_bytes: i64,
    #[serde(rename = "globalFiles", default, deserialize_with = "nullable")]
    pub global_files: i64,
    #[serde(rename = "globalBytes", default, deserialize_with = "nullable")]
    pub global_bytes: i64,
}

#[derive(Deserialize, Default)]
pub struct Connections {
    #[serde(default, deserialize_with = "nullable")]
    pub connections: BTreeMap<String, Connection>,
}

#[derive(Deserialize, Default, Clone)]
pub struct Connection {
    #[serde(default, deserialize_with = "nullable")]
    pub connected: bool,
    /// The remote device's Syncthing version (remote, untrusted text).
    #[serde(rename = "clientVersion", default, deserialize_with = "nullable")]
    pub client_version: String,
}

#[derive(Deserialize, Default, Clone)]
pub struct PendingFolder {
    #[serde(rename = "offeredBy", default, deserialize_with = "nullable")]
    pub offered_by: BTreeMap<String, FolderOffer>,
}

#[derive(Deserialize, Default, Clone)]
pub struct FolderOffer {
    #[serde(default, deserialize_with = "nullable")]
    pub label: String,
    #[serde(rename = "receiveEncrypted", default, deserialize_with = "nullable")]
    pub receive_encrypted: bool,
    #[serde(rename = "remoteEncrypted", default, deserialize_with = "nullable")]
    pub remote_encrypted: bool,
}

pub type PendingFolders = BTreeMap<String, PendingFolder>;

#[derive(Deserialize, Default, Clone)]
pub struct PendingDevice {
    #[serde(default, deserialize_with = "nullable")]
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub address: String,
}

#[derive(Deserialize, Default, Clone)]
pub struct DiscoveryEntry {
    #[serde(default, deserialize_with = "nullable")]
    pub addresses: Vec<String>,
}

#[derive(Deserialize, Default)]
pub struct GuiConfig {
    #[serde(default, deserialize_with = "nullable")]
    pub theme: String,
}

#[derive(Deserialize, Default)]
pub struct SystemPaths {
    #[serde(rename = "guiAssets", default, deserialize_with = "nullable")]
    pub gui_assets: String,
}

#[derive(Deserialize, Default)]
pub struct FolderErrors {
    #[serde(default, deserialize_with = "nullable")]
    pub errors: Vec<FolderError>,
}

#[derive(Deserialize, Default, Clone)]
pub struct FolderError {
    #[serde(default, deserialize_with = "nullable")]
    pub path: String,
    #[serde(default, deserialize_with = "nullable")]
    pub error: String,
}

#[derive(Deserialize, Default)]
struct RandomString {
    #[serde(default, deserialize_with = "nullable")]
    random: String,
}

#[derive(Deserialize, Default)]
pub struct FileInfo {
    #[serde(default)]
    pub local: Option<FileEntry>,
    #[serde(default)]
    pub global: Option<FileEntry>,
}

#[derive(Deserialize, Default, Clone)]
pub struct FileEntry {
    #[serde(rename = "type", default)]
    pub kind: Value,
    #[serde(default, deserialize_with = "nullable")]
    pub deleted: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Event {
    #[serde(default, deserialize_with = "nullable")]
    pub id: i64,
    #[serde(rename = "type", default, deserialize_with = "nullable")]
    pub kind: String,
    #[serde(default, deserialize_with = "present")]
    pub data: Option<Value>,
}

/// Whether a scan request finished or outlived its request (Syncshell
/// `RescanDisposition`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rescan {
    Completed,
    Running,
}

impl Client {
    pub fn new(
        transport: Transport,
        api_key: ApiKey,
        scheme: &'static str,
        endpoint: String,
    ) -> Client {
        Client {
            transport,
            api_key,
            scheme,
            endpoint,
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn request<T: DeserializeOwned + Default>(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        authenticated: bool,
        events: bool,
    ) -> Result<Option<T>, Error> {
        self.request_raw(method, path, body, authenticated, events, true)
            .and_then(|contents| contents.map(|bytes| decode::<T>(&bytes)).transpose())
    }

    /// The body (`None` when `want_body` is false), following same-origin
    /// redirects like Go's `http.Client`.
    fn request_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        authenticated: bool,
        events: bool,
        want_body: bool,
    ) -> Result<Option<Vec<u8>>, Error> {
        let mut method = method.to_string();
        let mut target = path.to_string();
        let mut body = body.map(<[u8]>::to_vec);
        let timeout = if events {
            EVENT_TIMEOUT
        } else {
            REQUEST_TIMEOUT
        };
        let started = std::time::Instant::now();
        for redirects in 0..=MAX_REDIRECTS + 1 {
            let mut headers: Vec<(&str, &str)> = vec![("Accept", "application/json")];
            if body.is_some() {
                headers.push(("Content-Type", "application/json"));
            }
            if authenticated {
                headers.push(("X-API-Key", &self.api_key.0));
            }
            let left = timeout.saturating_sub(started.elapsed());
            if left.is_zero() {
                return Err(Error::new(Code::Timeout, "request timed out"));
            }
            let response = self
                .transport
                .request(
                    &method,
                    &target,
                    &headers,
                    body.as_deref(),
                    left,
                    MAX_RESPONSE_BYTES,
                )
                .map_err(transport_error)?;
            if matches!(response.status, 301 | 302 | 303 | 307 | 308)
                && let Some(location) = response.location.as_deref()
            {
                if redirects == MAX_REDIRECTS {
                    // "redirect limit exceeded"
                    return Err(Error::new(
                        Code::Connection,
                        "could not connect to Syncthing",
                    ));
                }
                target = self.same_origin(location, &target).ok_or_else(|| {
                    Error::new(Code::Connection, "could not connect to Syncthing")
                })?;
                if matches!(response.status, 301..=303) && method != "GET" && method != "HEAD" {
                    method = "GET".into();
                    body = None;
                }
                continue;
            }
            if response.status == 401 || response.status == 403 {
                return Err(Error::new(
                    Code::Unauthorized,
                    "Syncthing authorization failed",
                ));
            }
            if !(200..300).contains(&response.status) {
                return Err(Error {
                    code: Code::Http,
                    message: format!("Syncthing returned HTTP {}", response.status),
                    status: response.status,
                });
            }
            let Some(contents) = response.body else {
                return Err(Error::new(Code::Schema, "Syncthing response is too large"));
            };
            return Ok(want_body.then_some(contents));
        }
        Err(Error::new(
            Code::Connection,
            "could not connect to Syncthing",
        ))
    }

    /// The request target of a same-origin `Location`, or `None` (Go's
    /// "cross-origin redirect rejected").
    fn same_origin(&self, location: &str, current: &str) -> Option<String> {
        if let Some((scheme, rest)) = location.split_once("://") {
            if !scheme.eq_ignore_ascii_case(self.scheme) {
                return None;
            }
            let (host, path) = rest.split_at(rest.find(['/', '?']).unwrap_or(rest.len()));
            if host != self.transport.host() {
                return None;
            }
            return Some(if path.is_empty() {
                "/".into()
            } else if path.starts_with('?') {
                format!("/{path}")
            } else {
                path.into()
            });
        }
        if location.starts_with("//") {
            return None;
        }
        if location.starts_with('/') {
            return Some(location.into());
        }
        let base = current.split('?').next().unwrap_or("/");
        let directory = &base[..base.rfind('/').map_or(0, |i| i + 1)];
        Some(format!("{directory}{location}"))
    }

    fn get<T: DeserializeOwned + Default>(&self, path: &str) -> Result<T, Error> {
        Ok(self
            .request("GET", path, None, true, false)?
            .unwrap_or_default())
    }

    fn send_json(&self, method: &str, path: &str, value: &impl Serialize) -> Result<(), Error> {
        let body = serde_json::to_vec(value)
            .map_err(|_| Error::new(Code::Schema, "could not encode Syncthing request"))?;
        self.request_raw(method, path, Some(&body), true, false, false)
            .map(|_| ())
    }

    /// Transport without the credential.
    pub fn health(&self) -> Result<(), Error> {
        #[derive(Deserialize, Default)]
        struct Health {
            #[serde(default, deserialize_with = "nullable")]
            status: String,
        }
        let health: Health = self
            .request("GET", "/rest/noauth/health", None, false, false)?
            .unwrap_or_default();
        if health.status != "OK" {
            return Err(Error::new(
                Code::Schema,
                "Syncthing health response is invalid",
            ));
        }
        Ok(())
    }

    pub fn status(&self) -> Result<SystemStatus, Error> {
        let status: SystemStatus = self.get("/rest/system/status")?;
        if status.my_id.trim().is_empty() {
            return Err(Error::new(Code::Schema, "Syncthing identity is missing"));
        }
        Ok(status)
    }

    pub fn version(&self) -> Result<SystemVersion, Error> {
        self.get("/rest/system/version")
    }

    pub fn devices(&self) -> Result<Vec<Device>, Error> {
        self.get("/rest/config/devices")
    }

    pub fn folders(&self) -> Result<Vec<Folder>, Error> {
        self.get("/rest/config/folders")
    }

    pub fn connections(&self) -> Result<Connections, Error> {
        self.get("/rest/system/connections")
    }

    pub fn folder_status(&self, folder_id: &str) -> Result<FolderStatus, Error> {
        self.get(&format!(
            "/rest/db/status?folder={}",
            query_escape(folder_id)
        ))
    }

    /// A new folder can be configured before its model serves errors: 404 is
    /// "no errors".
    pub fn folder_errors(&self, folder_id: &str) -> Result<FolderErrors, Error> {
        match self.get(&format!(
            "/rest/folder/errors?folder={}&page=1&perpage=100",
            query_escape(folder_id)
        )) {
            Err(Error {
                code: Code::Http,
                status: 404,
                ..
            }) => Ok(FolderErrors::default()),
            other => other,
        }
    }

    pub fn pending_folders(&self) -> Result<PendingFolders, Error> {
        self.get("/rest/cluster/pending/folders")
    }

    pub fn pending_devices(&self) -> Result<BTreeMap<String, PendingDevice>, Error> {
        self.get("/rest/cluster/pending/devices")
    }

    pub fn discovery_cache(&self) -> Result<BTreeMap<String, DiscoveryEntry>, Error> {
        self.get("/rest/system/discovery")
    }

    pub fn gui_config(&self) -> Result<GuiConfig, Error> {
        self.get("/rest/config/gui")
    }

    pub fn system_paths(&self) -> Result<SystemPaths, Error> {
        self.get("/rest/system/paths")
    }

    pub fn folder(&self, folder_id: &str) -> Result<Folder, Error> {
        self.get(&format!("/rest/config/folders/{}", path_escape(folder_id)))
    }

    pub fn set_folder_paused(&self, folder_id: &str, paused: bool) -> Result<(), Error> {
        let mut patch = BTreeMap::new();
        patch.insert("paused", paused);
        self.send_json(
            "PATCH",
            &format!("/rest/config/folders/{}", path_escape(folder_id)),
            &patch,
        )
    }

    pub fn delete_folder(&self, folder_id: &str) -> Result<(), Error> {
        self.request_raw(
            "DELETE",
            &format!("/rest/config/folders/{}", path_escape(folder_id)),
            None,
            true,
            false,
            false,
        )
        .map(|_| ())
    }

    pub fn default_folder(&self) -> Result<Map<String, Value>, Error> {
        self.get("/rest/config/defaults/folder")
    }

    pub fn add_folder(&self, config: &Map<String, Value>) -> Result<(), Error> {
        self.send_json("POST", "/rest/config/folders", config)
    }

    pub fn random_string(&self, length: usize) -> Result<String, Error> {
        let response: RandomString =
            self.get(&format!("/rest/svc/random/string?length={length}"))?;
        Ok(response.random)
    }

    pub fn file_info(&self, folder_id: &str, name: &str) -> Result<FileInfo, Error> {
        self.get(&format!(
            "/rest/db/file?folder={}&file={}",
            query_escape(folder_id),
            query_escape(name)
        ))
    }

    pub fn rescan_subdirectory(&self, folder_id: &str, subdirectory: &str) -> Result<(), Error> {
        let path = format!(
            "/rest/db/scan?folder={}&sub={}",
            query_escape(folder_id),
            query_escape(subdirectory)
        );
        self.request_raw("POST", &path, None, true, false, false)
            .map(|_| ())
    }

    /// One scan; a request that timed out while the folder (or, for all
    /// folders, any unpaused folder) is now scanning is `Running`.
    pub fn rescan(&self, folder_id: &str) -> Result<Rescan, Error> {
        let mut path = "/rest/db/scan".to_string();
        if !folder_id.is_empty() {
            path.push_str(&format!("?folder={}", query_escape(folder_id)));
        }
        match self.request_raw("POST", &path, None, true, false, false) {
            Ok(_) => Ok(Rescan::Completed),
            Err(error) if error.code == Code::Timeout && self.rescan_started(folder_id) => {
                Ok(Rescan::Running)
            }
            Err(error) => Err(error),
        }
    }

    fn rescan_started(&self, folder_id: &str) -> bool {
        if !folder_id.is_empty() {
            return self
                .folder_status(folder_id)
                .is_ok_and(|s| s.state.starts_with("scan"));
        }
        let Ok(folders) = self.folders() else {
            return false;
        };
        folders.iter().filter(|f| !f.paused).any(|f| {
            self.folder_status(&f.id)
                .is_ok_and(|s| s.state.starts_with("scan"))
        })
    }

    /// One filtered Event API long-poll.
    pub fn events(
        &self,
        since: i64,
        limit: usize,
        timeout_seconds: u64,
        types: &[&str],
    ) -> Result<Vec<Event>, Error> {
        // url.Values.Encode: keys sorted.
        let mut query = Vec::new();
        if !types.is_empty() {
            query.push(format!("events={}", query_escape(&types.join(","))));
        }
        query.push(format!("limit={limit}"));
        query.push(format!("since={since}"));
        query.push(format!("timeout={timeout_seconds}"));
        let path = format!("/rest/events?{}", query.join("&"));
        Ok(self
            .request::<Vec<Event>>("GET", &path, None, true, true)?
            .unwrap_or_default())
    }
}

/// Go's `json.Decoder.Decode` plus the trailing-data check.
fn decode<T: DeserializeOwned + Default>(bytes: &[u8]) -> Result<T, Error> {
    let mut stream = serde_json::Deserializer::from_slice(bytes).into_iter::<Option<T>>();
    let value = match stream.next() {
        Some(Ok(value)) => value.unwrap_or_default(),
        _ => return Err(Error::new(Code::Schema, "Syncthing response is invalid")),
    };
    let rest = &bytes[stream.byte_offset()..];
    if !rest.iter().all(u8::is_ascii_whitespace) {
        return Err(Error::new(
            Code::Schema,
            "Syncthing response contains trailing data",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unsupported_cpu_reports_the_kits_message() {
        let error = transport_error(http::Failure::TlsUnsupported);
        assert_eq!(
            (error.code, error.message.as_str()),
            (Code::Tls, foamy_helper::tls::UNSUPPORTED)
        );
    }

    #[test]
    fn decoding_is_as_lenient_as_go() {
        let status: SystemStatus = decode(br#" {"myID":"X","tilde":null,"extra":[1]} "#).unwrap();
        assert_eq!((status.my_id.as_str(), status.tilde.as_str()), ("X", ""));
        let folders: Vec<Folder> = decode(b"null").unwrap();
        assert!(folders.is_empty());
        let folders: Vec<Folder> = decode(br#"[{"id":"a","devices":null,"paused":null}]"#).unwrap();
        assert_eq!(
            (
                folders[0].id.as_str(),
                folders[0].paused,
                folders[0].devices.len()
            ),
            ("a", false, 0)
        );
        assert_eq!(
            decode::<SystemStatus>(b"").unwrap_err().message,
            "Syncthing response is invalid"
        );
        assert_eq!(
            decode::<SystemStatus>(b"{} {}").unwrap_err().message,
            "Syncthing response contains trailing data"
        );
        assert_eq!(
            decode::<SystemStatus>(br#"{"myID":5}"#).unwrap_err().code,
            Code::Schema
        );
        for bad in [
            &br#"[{"id":"a","devices":[{}]}]"#[..],
            br#"[{"devices":[{"deviceID":""}]}]"#,
            br#"[{"devices":[null]}]"#,
        ] {
            assert!(decode::<Vec<Folder>>(bad).is_err());
        }
        let events: Vec<Event> =
            decode(br#"[{"id":3,"type":"X"},{"id":4,"type":"Y","data":null}]"#).unwrap();
        assert_eq!(events[0].data, None);
        assert_eq!(events[1].data, Some(Value::Null));
    }

    #[test]
    fn folder_devices_serialize_with_their_id() {
        let device = FolderDevice {
            device_id: "A".into(),
        };
        assert_eq!(
            serde_json::to_string(&device).unwrap(),
            r#"{"deviceID":"A"}"#
        );
    }
}
