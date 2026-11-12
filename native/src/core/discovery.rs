//! Selecting exactly one Syncthing target (Syncshell
//! `internal/syncthing/discovery.go` and `endpoint.go`): `--config`, else an
//! explicit `--endpoint` with a private credential file, else the one local
//! `config.xml` (asking `syncthing paths` when there are several or none),
//! else SyncThingy's Flatpak configuration. The API key is read here and
//! handed straight to the client.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use foamy_helper::child::Bounded;
use quick_xml::events::Event;
use rustix::fs::{Mode, OFlags};

use super::client::{ApiKey, Code, Error};
use super::gocompat::{self, abs, clean, dir, join, regular_file, trim_space};

const MAX_CONFIG_BYTES: u64 = 1 << 20;
const MAX_CREDENTIAL_BYTES: u64 = 4096;
const MAX_PATHS_OUTPUT: usize = 64 << 10;

#[derive(Default)]
pub struct Options {
    pub config_path: String,
    pub endpoint: String,
    pub credential_file: String,
    pub tls_certificate: String,
    pub expected_device_id: String,
    pub insecure_tls: bool,
}

/// The selected endpoint. Only `api_key` is secret.
pub struct Target {
    /// `http://host:port`, `https://host:port`, `unix:///path` or `unixs:///path`.
    pub endpoint: String,
    pub config_path: String,
    pub expected_device_id: String,
    /// Loopback or a Unix socket.
    pub local: bool,
    /// Found by automatic local discovery (not Flatpak, not explicit).
    pub automatic: bool,
    pub api_key: ApiKey,
    /// The socket path for a `unix`/`unixs` endpoint, else empty.
    pub unix_socket: OsString,
    pub tls_certificate: String,
    pub insecure_tls: bool,
}

fn failure(code: Code, message: &str) -> Error {
    Error::new(code, message)
}

pub fn discover(options: &Options) -> Result<Target, Error> {
    if !options.config_path.is_empty() {
        return target_from_config(&options.config_path, &options.expected_device_id);
    }
    if !options.endpoint.is_empty() {
        return target_from_explicit(options);
    }
    let mut paths = existing(&candidate_config_paths());
    let mut native = true;
    if paths.len() != 1
        && let Some(path) = syncthing_config_path()
        && regular_file(&path)
    {
        paths = vec![path];
    }
    if paths.is_empty() {
        paths = existing(&sync_thingy_config_paths());
        native = false;
    }
    match paths.len() {
        0 => Err(failure(
            Code::Config,
            "Syncthing configuration was not found",
        )),
        1 => {
            let mut target = target_from_config(&paths[0], &options.expected_device_id)?;
            // Flatpak discovery must not authorize the default native user service.
            target.automatic = native;
            Ok(target)
        }
        _ => Err(failure(
            Code::Ambiguity,
            "multiple Syncthing configurations were found",
        )),
    }
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn candidate_config_paths() -> Vec<String> {
    let home = home();
    let mut config_home = env("XDG_CONFIG_HOME");
    if config_home.is_empty() && !home.is_empty() {
        config_home = join(&[&home, ".config"]);
    }
    let mut state_home = env("XDG_STATE_HOME");
    if state_home.is_empty() && !home.is_empty() {
        state_home = join(&[&home, ".local", "state"]);
    }
    let mut paths = Vec::new();
    for variable in ["STCONFDIR", "STHOMEDIR"] {
        let value = env(variable);
        if !value.is_empty() {
            paths.push(join(&[&value, "config.xml"]));
        }
    }
    paths.push(join(&[&state_home, "syncthing", "config.xml"]));
    paths.push(join(&[&config_home, "syncthing", "config.xml"]));
    paths.push(join(&[&home, ".local", "share", "syncthing", "config.xml"]));
    let mut seen = Vec::new();
    for path in paths {
        if path.is_empty() {
            continue;
        }
        let path = clean(&path);
        if !seen.contains(&path) {
            seen.push(path);
        }
    }
    seen
}

fn sync_thingy_config_paths() -> Vec<String> {
    let root = join(&[&home(), ".var", "app", "com.github.zocker_160.SyncThingy"]);
    vec![
        join(&[&root, ".local", "state", "syncthing", "config.xml"]),
        join(&[&root, "config", "syncthing", "config.xml"]),
    ]
}

fn existing(paths: &[String]) -> Vec<String> {
    paths.iter().filter(|p| regular_file(p)).cloned().collect()
}

/// `syncthing paths` (3 s, 64 KiB): the line after "Configuration file:".
fn syncthing_config_path() -> Option<String> {
    let binary = gocompat::look_path("syncthing")?;
    let output = Bounded::new(binary)
        .arg("paths")
        .timeout(Duration::from_secs(3))
        .max_output(MAX_PATHS_OUTPUT)
        .run()
        .ok()?;
    if !output.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.split('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() != "Configuration file:" || index + 1 >= lines.len() {
            continue;
        }
        let path = lines[index + 1].trim();
        if path.is_empty() {
            return None;
        }
        return Some(clean(path));
    }
    None
}

/// `FindExecutable("")`: `syncthing` on `PATH`, absolute, or empty.
pub fn find_executable() -> String {
    gocompat::look_path("syncthing")
        .map(|p| clean(&p))
        .unwrap_or_default()
}

fn target_from_config(path: &str, expected_device_id: &str) -> Result<Target, Error> {
    let clean_path = abs(path).map_err(|_| failure(Code::Config, "invalid configuration path"))?;
    let file = fs::File::open(&clean_path)
        .map_err(|_| failure(Code::Config, "could not read Syncthing configuration"))?;
    let mut contents = Vec::new();
    let read = file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut contents);
    if read.is_err() || contents.len() as u64 > MAX_CONFIG_BYTES {
        return Err(failure(
            Code::Config,
            "Syncthing configuration is too large",
        ));
    }
    let gui = parse_config(&contents)
        .ok_or_else(|| failure(Code::Config, "Syncthing configuration is malformed"))?;
    let api_key = trim_space(&gui.api_key).to_string();
    if api_key.is_empty() || api_key.contains(['\r', '\n', '\0']) {
        return Err(failure(
            Code::Credential,
            "Syncthing API credential is missing",
        ));
    }
    let (endpoint, unix_socket, local) = normalize_endpoint(&gui.address, gui.tls)?;
    let mut tls_certificate = String::new();
    if endpoint.starts_with("https://") || endpoint.starts_with("unixs://") {
        let certificate = join(&[&dir(&clean_path), "https-cert.pem"]);
        if !regular_file(&certificate) {
            return Err(failure(Code::Tls, "local HTTPS certificate was not found"));
        }
        tls_certificate = certificate;
    }
    Ok(Target {
        endpoint,
        config_path: clean_path,
        expected_device_id: trim_space(expected_device_id).to_string(),
        local,
        automatic: false,
        api_key: ApiKey::new(api_key),
        unix_socket,
        tls_certificate,
        insecure_tls: false,
    })
}

fn target_from_explicit(options: &Options) -> Result<Target, Error> {
    if options.credential_file.is_empty() {
        return Err(failure(
            Code::Credential,
            "an explicit target requires a protected credential file",
        ));
    }
    let api_key = read_protected_credential(&options.credential_file)?;
    let (endpoint, unix_socket, local) = normalize_endpoint(&options.endpoint, false)?;
    let https = endpoint.starts_with("https://") || endpoint.starts_with("unixs://");
    if options.insecure_tls && !https {
        return Err(failure(
            Code::Tls,
            "insecure TLS is valid only for an HTTPS target",
        ));
    }
    let mut tls_certificate = String::new();
    if !options.tls_certificate.is_empty() {
        tls_certificate = abs(&options.tls_certificate).unwrap_or_default();
        if tls_certificate.is_empty() || !regular_file(&tls_certificate) {
            return Err(failure(Code::Tls, "TLS certificate was not found"));
        }
    }
    Ok(Target {
        endpoint,
        config_path: String::new(),
        expected_device_id: trim_space(&options.expected_device_id).to_string(),
        local,
        automatic: false,
        api_key: ApiKey::new(api_key),
        unix_socket,
        tls_certificate,
        insecure_tls: options.insecure_tls,
    })
}

/// A regular file (not a link) readable by its owner only, at most 4 KiB.
///
/// The path is opened once and everything after that uses the descriptor:
/// the checks are on the open file (`fstat`), and the bounded read is from
/// the same file, so a rename or symlink swapped in after the check can't
/// substitute another file, and an oversized one is never buffered whole.
fn read_protected_credential(path: &str) -> Result<String, Error> {
    let private = || {
        failure(
            Code::Credential,
            "credential file must be a private regular file",
        )
    };
    // NOFOLLOW: a symlink in the last component fails (ELOOP). NONBLOCK: a
    // FIFO planted there can't hang the open; it fails the regular-file check.
    let flags =
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC;
    let file = fs::File::from(rustix::fs::open(path, flags, Mode::empty()).map_err(|_| private())?);
    let metadata = file.metadata().map_err(|_| private())?;
    if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(private());
    }
    let mut contents = Vec::new();
    file.take(MAX_CREDENTIAL_BYTES + 1)
        .read_to_end(&mut contents)
        .map_err(|_| failure(Code::Credential, "could not read credential file"))?;
    if contents.len() as u64 > MAX_CREDENTIAL_BYTES {
        return Err(failure(Code::Credential, "credential file is too large"));
    }
    let text = String::from_utf8_lossy(&contents);
    let credential = trim_space(&text).to_string();
    if credential.is_empty() || credential.contains(['\r', '\n', '\0']) {
        return Err(failure(Code::Credential, "credential file is invalid"));
    }
    Ok(credential)
}

/// The parsed `<gui>` settings; `api_key` is the credential, so `Debug`
/// shows only whether there is one.
#[derive(Default, PartialEq)]
pub struct Gui {
    pub tls: bool,
    pub address: String,
    pub api_key: String,
}

impl std::fmt::Debug for Gui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let api_key = if self.api_key.is_empty() {
            "empty"
        } else {
            "redacted"
        };
        f.debug_struct("Gui")
            .field("tls", &self.tls)
            .field("address", &self.address)
            .field("api_key", &format_args!("{api_key}"))
            .finish()
    }
}

/// Go's `xml.Unmarshal` into `{GUI {TLS attr; Address; APIKey} "gui"}`: the
/// root's direct `<gui>` children, later ones overriding what they set; an
/// element's own text (character data, CDATA, references), not that of
/// nested elements; anything after the root is ignored. `None` for
/// malformed XML, a non-UTF-8 encoding or an invalid `tls` value.
pub fn parse_config(contents: &[u8]) -> Option<Gui> {
    let mut reader = quick_xml::Reader::from_reader(contents);
    reader.config_mut().check_end_names = true;
    reader.config_mut().trim_text(false);
    let mut gui = Gui::default();
    let mut depth = 0usize;
    // A <gui> child of the root is open.
    let mut in_gui = false;
    // The <address> or <apikey> child of that <gui> and its text so far.
    let mut field: Option<(bool, String)> = None;
    let mut buffer = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buffer).ok()?;
        let (start, empty) = match &event {
            Event::Start(start) => (Some(start), false),
            Event::Empty(start) => (Some(start), true),
            _ => (None, false),
        };
        if let Some(start) = start {
            depth += 1;
            let local = start.local_name();
            let name = local.as_ref();
            if depth == 2 {
                in_gui = name == b"gui";
                if in_gui {
                    for attribute in start.attributes().with_checks(false) {
                        let attribute = attribute.ok()?;
                        if attribute.key.local_name().as_ref() == b"tls" {
                            gui.tls = parse_bool(trim_space(&attribute.unescape_value().ok()?))?;
                        }
                    }
                }
            } else if depth == 3 && in_gui {
                field = match name {
                    b"address" => Some((true, String::new())),
                    b"apikey" => Some((false, String::new())),
                    _ => None,
                };
            }
            if !empty {
                buffer.clear();
                continue;
            }
        }
        match event {
            Event::Eof => return None,
            Event::Decl(decl) => {
                if let Some(encoding) = decl.encoding()
                    && !encoding.ok()?.eq_ignore_ascii_case(b"utf-8")
                {
                    return None;
                }
            }
            Event::Start(_) => {}
            Event::Empty(_) | Event::End(_) => {
                if depth == 3
                    && let Some((address, value)) = field.take()
                {
                    if address {
                        gui.address = value;
                    } else {
                        gui.api_key = value;
                    }
                }
                if depth == 2 {
                    in_gui = false;
                }
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(gui);
                }
            }
            Event::Text(text) => {
                let text = text.decode().ok()?;
                if depth == 3
                    && let Some((_, value)) = field.as_mut()
                {
                    value.push_str(&text);
                }
            }
            Event::CData(data) => {
                let text = data.decode().ok()?;
                if depth == 3
                    && let Some((_, value)) = field.as_mut()
                {
                    value.push_str(&text);
                }
            }
            Event::GeneralRef(reference) => {
                let resolved = if reference.is_char_ref() {
                    reference.resolve_char_ref().ok()??
                } else {
                    match reference.decode().ok()?.as_ref() {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "apos" => '\'',
                        "quot" => '"',
                        _ => return None,
                    }
                };
                if depth == 3
                    && let Some((_, value)) = field.as_mut()
                {
                    value.push(resolved);
                }
            }
            Event::Comment(_) | Event::PI(_) | Event::DocType(_) => {}
        }
        buffer.clear();
    }
}

/// `strconv.ParseBool`.
fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "" => Some(false),
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// `normalizeEndpoint`: (endpoint, unix socket path, local).
pub fn normalize_endpoint(
    value: &str,
    tls_enabled: bool,
) -> Result<(String, OsString, bool), Error> {
    let address = trim_space(value);
    if address.is_empty() {
        return Err(failure(Code::Config, "Syncthing GUI address is missing"));
    }
    if gocompat::is_abs(address) {
        return Ok((format!("unix://{address}"), address.into(), true));
    }
    if address.contains("://") {
        return normalize_url(address);
    }
    let (host, port) = split_host_port(address)
        .ok_or_else(|| failure(Code::Config, "Syncthing GUI address is invalid"))?;
    let host = normalize_host(host);
    let scheme = if tls_enabled { "https" } else { "http" };
    Ok((
        format!("{scheme}://{}", join_host_port(&host, port)),
        OsString::new(),
        is_loopback(&host),
    ))
}

/// `normalizeURL`, with `url.Parse`'s errors (control bytes, a malformed
/// scheme, a broken `%` escape in the path or fragment) as "invalid URL".
/// The path is used decoded, as Go's `URL.Path` is: `unix:///tmp/gui%20socket`
/// is the socket `/tmp/gui socket`. Unlike Go, a decoded NUL is refused
/// (Go would hand the kernel a path that ends at the NUL).
fn normalize_url(value: &str) -> Result<(String, OsString, bool), Error> {
    let invalid = || failure(Code::Config, "Syncthing target URL is invalid");
    let (value, fragment) = match value.split_once('#') {
        Some((value, fragment)) => (value, Some(fragment)),
        None => (value, None),
    };
    if value.bytes().any(|b| b < b' ' || b == 0x7f) {
        return Err(invalid());
    }
    let (scheme, rest) = value.split_once("://").ok_or_else(invalid)?;
    let scheme = scheme.to_ascii_lowercase();
    if !scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
    {
        return Err(invalid());
    }
    let (rest, query) = match rest.split_once('?') {
        Some((rest, query)) => (rest, Some(query)),
        None => (rest, None),
    };
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = gocompat::unescape(path).ok_or_else(invalid)?;
    if fragment.is_some_and(|fragment| gocompat::unescape(fragment).is_none()) {
        return Err(invalid());
    }
    let clean_extras = query.is_none_or(str::is_empty) && fragment.is_none_or(str::is_empty);
    match scheme.as_str() {
        "unix" | "unixs" => {
            if !path.starts_with(b"/")
                || path.contains(&0)
                || !clean_extras
                || !authority.is_empty()
            {
                return Err(failure(Code::Config, "Syncthing Unix target is invalid"));
            }
            let endpoint = format!("{scheme}://{}", gocompat::lossy(&path));
            Ok((endpoint, OsString::from_vec(path), true))
        }
        "http" | "https" => {
            if authority.contains('@')
                || authority.is_empty()
                || !(path.is_empty() || path == b"/")
                || !clean_extras
            {
                return Err(invalid());
            }
            let (hostname, port) = match authority.strip_prefix('[') {
                Some(bracketed) => {
                    let (host, after) = bracketed.split_once(']').ok_or_else(invalid)?;
                    let port = match after {
                        "" => "",
                        _ => after.strip_prefix(':').ok_or_else(invalid)?,
                    };
                    (host, port)
                }
                None => match authority.rsplit_once(':') {
                    Some((host, port)) if !host.contains(':') => (host, port),
                    Some(_) => return Err(invalid()),
                    None => (authority, ""),
                },
            };
            if !port.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            let hostname = normalize_host(hostname);
            let port = if !port.is_empty() {
                port
            } else if scheme == "https" {
                "443"
            } else {
                "80"
            };
            Ok((
                format!("{scheme}://{}", join_host_port(&hostname, port)),
                OsString::new(),
                is_loopback(&hostname),
            ))
        }
        _ => Err(failure(
            Code::Config,
            "Syncthing target scheme is unsupported",
        )),
    }
}

/// `net.SplitHostPort`.
fn split_host_port(hostport: &str) -> Option<(&str, &str)> {
    let last = hostport.rfind(':')?;
    let (host, start, end) = if hostport.starts_with('[') {
        let close = hostport.find(']')?;
        if close + 1 != last {
            return None;
        }
        (&hostport[1..close], 1, close + 1)
    } else {
        let host = &hostport[..last];
        if host.contains(':') {
            return None;
        }
        (host, 0, 0)
    };
    if hostport[start..].contains('[') || hostport[end..].contains(']') {
        return None;
    }
    Some((host, &hostport[last + 1..]))
}

fn join_host_port(host: &str, port: &str) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn normalize_host(host: &str) -> String {
    match host {
        "" | "0.0.0.0" | "*" => "127.0.0.1".into(),
        "::" | "[::]" => "::1".into(),
        other => other.trim_matches(['[', ']']).into(),
    }
}

pub fn is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback(),
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn credential_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("foamy-credential-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        dir
    }

    fn write_mode(path: &std::path::Path, text: &[u8], mode: u32) {
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn message(result: Result<String, Error>) -> String {
        match result {
            Ok(text) => format!("ok {text}"),
            Err(error) => error.message,
        }
    }

    #[test]
    fn a_credential_file_must_be_private_regular_and_small() {
        let dir = credential_dir("rules");
        let read =
            |name: &str| message(read_protected_credential(dir.join(name).to_str().unwrap()));
        write_mode(&dir.join("ok"), b" key-Canary-1 \n", 0o600);
        assert_eq!(read("ok"), "ok key-Canary-1");
        write_mode(&dir.join("public"), b"key", 0o644);
        assert_eq!(
            read("public"),
            "credential file must be a private regular file"
        );
        symlink(dir.join("ok"), dir.join("link")).unwrap();
        assert_eq!(
            read("link"),
            "credential file must be a private regular file"
        );
        assert_eq!(
            read("missing"),
            "credential file must be a private regular file"
        );
        fs::create_dir(dir.join("dir")).unwrap();
        fs::set_permissions(dir.join("dir"), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            read("dir"),
            "credential file must be a private regular file"
        );
        // A FIFO with no writer would block a plain open forever.
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            dir.join("fifo"),
            Mode::from_raw_mode(0o600),
        )
        .unwrap();
        assert_eq!(
            read("fifo"),
            "credential file must be a private regular file"
        );
        write_mode(
            &dir.join("limit"),
            &vec![b'k'; MAX_CREDENTIAL_BYTES as usize],
            0o600,
        );
        assert_eq!(read("limit").len(), 3 + MAX_CREDENTIAL_BYTES as usize);
        write_mode(
            &dir.join("large"),
            &vec![b'k'; MAX_CREDENTIAL_BYTES as usize + 1],
            0o600,
        );
        assert_eq!(read("large"), "credential file is too large");
        write_mode(&dir.join("lines"), b"a\nb", 0o600);
        assert_eq!(read("lines"), "credential file is invalid");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A private file checked and then swapped (rename over the path) for a
    /// symlink to a public file must never yield the public file's text.
    #[test]
    fn a_file_swapped_in_after_the_check_is_never_read() {
        let dir = credential_dir("swap");
        let path = dir.join("credential");
        write_mode(&dir.join("private"), b"private-key", 0o600);
        write_mode(&dir.join("public"), b"public-Canary-7", 0o644);
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let swapper = {
            let (dir, path, stop) = (dir.clone(), path.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut round = 0u64;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    round += 1;
                    let staged = dir.join(format!("staged-{}", round % 2));
                    let _ = fs::remove_file(&staged);
                    if round.is_multiple_of(2) {
                        fs::copy(dir.join("private"), &staged).unwrap();
                        fs::set_permissions(&staged, fs::Permissions::from_mode(0o600)).unwrap();
                    } else {
                        symlink(dir.join("public"), &staged).unwrap();
                    }
                    fs::rename(&staged, &path).unwrap();
                }
            })
        };
        let started = std::time::Instant::now();
        let mut read = 0;
        while started.elapsed() < Duration::from_secs(2) {
            if let Ok(text) = read_protected_credential(path.to_str().unwrap()) {
                assert_eq!(text, "private-key", "read through a swapped-in link");
                read += 1;
            }
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        swapper.join().unwrap();
        assert!(read > 0, "the private file was never read");
        fs::remove_dir_all(&dir).unwrap();
    }

    fn gui(text: &str) -> Option<(bool, String, String)> {
        parse_config(text.as_bytes()).map(|g| (g.tls, g.address, g.api_key))
    }

    #[test]
    fn debug_output_never_shows_the_api_key() {
        let key = "debug-Canary-31337";
        let config = format!(
            r#"<configuration><gui tls="true"><address>127.0.0.1:8384</address><apikey>{key}</apikey></gui></configuration>"#
        );
        let gui = parse_config(config.as_bytes()).unwrap();
        assert_eq!(gui.api_key, key);
        for text in [
            format!("{gui:?}"),
            format!("{gui:#?}"),
            format!("{:?}", Some(&gui)),
        ] {
            assert!(!text.contains(key), "{text}");
            assert!(
                text.contains("redacted") && text.contains("127.0.0.1:8384"),
                "{text}"
            );
        }
        assert!(format!("{:?}", Gui::default()).contains("empty"));
        let target = ApiKey::new(key.to_string());
        assert!(!format!("{target:?}").contains(key));
    }

    #[test]
    fn config_xml_matches_go_encoding_xml() {
        let some =
            |tls, address: &str, key: &str| Some((tls, address.to_string(), key.to_string()));
        assert_eq!(
            gui(r#"<c><gui tls=""><address>a</address></gui></c>"#),
            some(false, "a", "")
        );
        assert_eq!(
            gui(r#"<c><gui tls=" true "><address>a</address></gui></c>"#),
            some(true, "a", "")
        );
        assert_eq!(gui(r#"<c><gui tls="TRUE"/></c>"#), some(true, "", ""));
        assert_eq!(gui(r#"<c><gui tls="1"/></c>"#), some(true, "", ""));
        assert_eq!(gui(r#"<c><gui tls="yes"/></c>"#), None);
        assert_eq!(
            gui(
                r#"<c><gui tls="true"><address>a</address><apikey>k</apikey></gui><gui><address>b</address></gui></c>"#
            ),
            some(true, "b", "k")
        );
        assert_eq!(
            gui("<c><gui><address>a<b>x</b>c<![CDATA[d]]>&amp;&#65;</address></gui></c>"),
            some(false, "acd&A", "")
        );
        assert_eq!(
            gui(r#"<c><x:gui xmlns:x="u"><address>n</address></x:gui></c>"#),
            some(false, "n", "")
        );
        assert_eq!(
            gui("<c><other><gui><address>deep</address></gui></other></c>"),
            some(false, "", "")
        );
        assert_eq!(
            gui(r#"<?xml version="1.0" encoding="ISO-8859-1"?><c><gui/></c>"#),
            None
        );
        assert_eq!(
            gui(r#"<?xml version="1.0" encoding="utf-8"?><c><gui><address>a</address></gui></c>"#),
            some(false, "a", "")
        );
        assert_eq!(gui("<c><gui><address>&nope;</address></gui></c>"), None);
        assert_eq!(
            gui("<c><gui><address>a</address></gui></c>trailing<garbage"),
            some(false, "a", "")
        );
        assert_eq!(gui("<c><gui><address>a</address></gui>"), None);
        assert_eq!(
            gui(r#"<!DOCTYPE c [<!ENTITY e "x">]><c><gui><address>&e;</address></gui></c>"#),
            None
        );
        assert_eq!(gui("<c><gui><address>a</address></gui></d>"), None);
        assert_eq!(gui(r#"<c a="1" a="2"><gui/></c>"#), some(false, "", ""));
        assert_eq!(gui(""), None);
        assert_eq!(
            gui("  <!-- c --> <c><gui><apikey> k </apikey></gui></c>"),
            some(false, "", " k ")
        );
        assert_eq!(
            gui(r#"<c><gui tls="true"><address>a</address></gui><gui tls="false"/></c>"#),
            some(false, "a", "")
        );
        assert_eq!(
            gui("<c><address>top</address><gui></gui></c>"),
            some(false, "", "")
        );
        assert_eq!(gui("<c/>"), some(false, "", ""));
    }

    #[test]
    fn endpoints_normalize_like_syncshell() {
        let ok = |value: &str, tls: bool| normalize_endpoint(value, tls).unwrap();
        assert_eq!(
            ok("127.0.0.1:8384", false),
            ("http://127.0.0.1:8384".into(), OsString::new(), true)
        );
        assert_eq!(
            ok("0.0.0.0:8384", true),
            ("https://127.0.0.1:8384".into(), OsString::new(), true)
        );
        assert_eq!(ok(":8384", false).0, "http://127.0.0.1:8384");
        assert_eq!(
            ok("[::]:8384", false),
            ("http://[::1]:8384".into(), OsString::new(), true)
        );
        assert_eq!(ok("[::1]:8384", false).0, "http://[::1]:8384");
        assert!(ok("localhost:8384", false).2);
        assert!(!ok("192.168.1.2:8384", false).2);
        assert_eq!(
            ok("/run/st.sock", true),
            ("unix:///run/st.sock".into(), "/run/st.sock".into(), true)
        );
        assert_eq!(
            ok("unixs:///run/st.sock", false),
            ("unixs:///run/st.sock".into(), "/run/st.sock".into(), true)
        );
        assert_eq!(ok("HTTPS://[::1]", false).0, "https://[::1]:443");
        assert_eq!(
            ok("http://example.org/", false),
            ("http://example.org:80".into(), OsString::new(), false)
        );
        // The URL path is used decoded, as Go's URL.Path (outputs checked
        // against Syncshell's normalizeEndpoint built with Go 1.27).
        for (value, endpoint, socket) in [
            (
                "unix:///tmp/gui%20socket",
                "unix:///tmp/gui socket",
                &b"/tmp/gui socket"[..],
            ),
            (
                "unixs:///run/st%2Fx.sock",
                "unixs:///run/st/x.sock",
                b"/run/st/x.sock",
            ),
            ("UNIX:///tmp/a+b?", "unix:///tmp/a+b", b"/tmp/a+b"),
            ("unix:///tmp/x#", "unix:///tmp/x", b"/tmp/x"),
            ("unix:///%2e%2e/x", "unix:///../x", b"/../x"),
            (
                "unix:///tmp/%e6%C3%A6",
                "unix:///tmp/\u{FFFD}\u{e6}",
                b"/tmp/\xe6\xc3\xa6",
            ),
            ("unix:///tmp/%ff", "unix:///tmp/\u{FFFD}", b"/tmp/\xff"),
        ] {
            assert_eq!(
                ok(value, false),
                (endpoint.into(), OsString::from_vec(socket.to_vec()), true),
                "{value}"
            );
        }
        // A raw absolute path is taken as it is.
        assert_eq!(
            ok("/tmp/gui%20socket", false),
            (
                "unix:///tmp/gui%20socket".into(),
                "/tmp/gui%20socket".into(),
                true
            )
        );
        for (value, message) in [
            ("", "Syncthing GUI address is missing"),
            ("nope", "Syncthing GUI address is invalid"),
            ("a:b:c", "Syncthing GUI address is invalid"),
            ("[::1]8384", "Syncthing GUI address is invalid"),
            ("http://u@h:1", "Syncthing target URL is invalid"),
            ("http://h:1/x", "Syncthing target URL is invalid"),
            ("http://h:1?q", "Syncthing target URL is invalid"),
            ("unix://relative", "Syncthing Unix target is invalid"),
            ("ftp://h:1", "Syncthing target scheme is unsupported"),
            // url.Parse errors: a broken escape, a control byte, a bad scheme.
            ("unix:///tmp/%zz", "Syncthing target URL is invalid"),
            ("unix:///tmp/%2", "Syncthing target URL is invalid"),
            ("unix:///tmp/x%", "Syncthing target URL is invalid"),
            ("unix:///tmp/x#%zz", "Syncthing target URL is invalid"),
            ("unix:///tmp/a\tb", "Syncthing target URL is invalid"),
            ("unix:///tmp/a\x7fb", "Syncthing target URL is invalid"),
            ("1unix:///tmp/a", "Syncthing target URL is invalid"),
            ("http://h:1/%zz", "Syncthing target URL is invalid"),
            ("http://h:1/%2F", "Syncthing target URL is invalid"),
            ("http://h:1#%zz", "Syncthing target URL is invalid"),
            ("ftp://h/%zz", "Syncthing target URL is invalid"),
            // Refused here, unlike Go: a socket path with a NUL in it.
            ("unix:///tmp/a%00b", "Syncthing Unix target is invalid"),
        ] {
            assert_eq!(
                normalize_endpoint(value, false).unwrap_err().message,
                message,
                "{value}"
            );
        }
        assert!(is_loopback("::ffff:127.0.0.1"));
        assert!(is_loopback("127.1.2.3"));
        assert!(!is_loopback("127.000.000.001"));
    }
}
