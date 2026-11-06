//! `stream`: the native core, a Rust port of Syncshell's core.
//!
//! Syncshell (<https://github.com/omarchy-QOL/syncshell>, MIT, by IlyaZar)
//! is the upstream project; this module follows its `core/` at commit
//! `7318f1404e765f274df3d079e35d0ad32b5c1701` (v0.1.9), the protocol the
//! plugin has always spoken, and keeps its names where it can so a fix
//! upstream can be found here. Only what Foamy Syncthing uses is ported (see
//! the crate documentation).

pub mod actions;
pub mod activity;
// The REST calls and path checks the actions use arrive with them in the
// next commits.
#[allow(dead_code)]
pub mod client;
pub mod discovery;
pub mod events;
#[allow(dead_code)]
pub mod gocompat;
pub mod http;
pub mod protocol;
pub mod session;
pub mod systemd;
pub mod tls;
pub mod types;

use std::ffi::OsString;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use client::{Client, Code, Error};
use discovery::{Options, Target};

/// The `hello` build version: this port and the Syncshell release it follows.
pub const BUILD_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "+syncshell.0.1.9");

pub fn main(args: &[OsString]) -> ExitCode {
    // No core dump of a process that holds the API key.
    let _ = foamy_helper::process::limit(foamy_helper::process::Resource::Core, 0);
    match start(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("foamy-syncthing-helper: {message}");
            ExitCode::from(1)
        }
    }
}

fn start(args: &[OsString]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let target = discovery::discover(&flags.discovery).map_err(|e| e.message)?;
    let lifecycle_target = systemd::Target {
        config_path: target.config_path.clone(),
        local: target.local,
        automatic: target.automatic,
    };
    let expected_device_id = target.expected_device_id.clone();
    let client = new_client(target).map_err(|e| e.message)?;
    let session = session::Session::new(
        client,
        lifecycle_target,
        expected_device_id,
        flags.binding,
        discovery::find_executable(),
        session::Operational {
            probe_interval: Duration::from_secs(flags.probe_seconds as u64),
            refresh_interval: Duration::from_secs(60),
            desired_service_state: flags.desired_service_state,
        },
    );
    protocol::run(Arc::new(session), BUILD_VERSION)
}

/// One bounded transport to the target (Syncshell `NewClient`).
fn new_client(target: Target) -> Result<Client, Error> {
    let invalid = || Error::new(Code::Config, "Syncthing endpoint is invalid");
    let (scheme, rest) = target.endpoint.split_once("://").ok_or_else(invalid)?;
    let (address, scheme) = match scheme {
        "unix" => (http::Address::Unix(target.unix_socket.clone()), "http"),
        "unixs" => (http::Address::Unix(target.unix_socket.clone()), "https"),
        "http" | "https" => {
            let (host, port) = rest.rsplit_once(':').ok_or_else(invalid)?;
            let host = host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            let port: u16 = port.parse().map_err(|_| invalid())?;
            (
                http::Address::Tcp { host, port },
                if scheme == "https" { "https" } else { "http" },
            )
        }
        _ => return Err(invalid()),
    };
    let tls = if scheme == "https" {
        let mode = if target.insecure_tls {
            tls::Mode::Insecure
        } else if target.tls_certificate.is_empty() {
            tls::Mode::SystemRoots
        } else {
            let contents = std::fs::read(&target.tls_certificate)
                .map_err(|_| Error::new(Code::Tls, "could not read TLS certificate"))?;
            tls::pin_from_pem(&contents)
                .ok_or_else(|| Error::new(Code::Tls, "TLS certificate is invalid"))?
        };
        Some(
            tls::client_config(mode)
                .map_err(|_| Error::new(Code::Tls, "TLS configuration failed"))?,
        )
    } else {
        None
    };
    Ok(Client::new(
        http::Transport::new(address, tls),
        target.api_key,
        scheme,
        target.endpoint,
    ))
}

struct Flags {
    discovery: Options,
    binding: systemd::Binding,
    probe_seconds: i64,
    desired_service_state: String,
}

/// Go's `flag` package rules: `-name` or `--name`, a value as `=value` or
/// the next argument (booleans only as `=value`), parsing stops at the first
/// non-flag or `--`, the last repetition wins. Then Syncshell's checks.
fn parse_flags(args: &[OsString]) -> Result<Flags, String> {
    let invalid = || "invalid command arguments".to_string();
    let mut strings: std::collections::BTreeMap<&str, String> = std::collections::BTreeMap::new();
    let mut bools: std::collections::BTreeMap<&str, bool> = std::collections::BTreeMap::new();
    const STRING_FLAGS: [&str; 9] = [
        "config",
        "endpoint",
        "credential-file",
        "tls-certificate",
        "expected-device-id",
        "lifecycle-kind",
        "lifecycle-unit",
        "lifecycle-config",
        "desired-service-state",
    ];
    const BOOL_FLAGS: [&str; 3] = ["insecure-tls", "lifecycle-authorized", "json"];
    let mut probe = "15".to_string();
    let mut rest = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_str().ok_or_else(invalid)?;
        index += 1;
        if arg.len() < 2 || !arg.starts_with('-') {
            rest.push(arg);
            rest.extend(args[index..].iter().map(|a| a.to_str().unwrap_or("")));
            break;
        }
        let mut name = &arg[1..];
        if let Some(stripped) = name.strip_prefix('-') {
            if stripped.is_empty() {
                rest.extend(args[index..].iter().map(|a| a.to_str().unwrap_or("")));
                break;
            }
            name = stripped;
        }
        if name.is_empty() || name.starts_with('-') || name.starts_with('=') {
            return Err(invalid());
        }
        let (name, value) = match name.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (name, None),
        };
        if let Some(flag) = BOOL_FLAGS.iter().find(|f| **f == name) {
            let value = match value {
                None => true,
                Some(text) => parse_go_bool(text).ok_or_else(invalid)?,
            };
            bools.insert(flag, value);
            continue;
        }
        let value = match value {
            Some(value) => value.to_string(),
            None => {
                let next = args
                    .get(index)
                    .and_then(|a| a.to_str())
                    .ok_or_else(invalid)?;
                index += 1;
                next.to_string()
            }
        };
        if name == "probe-interval-seconds" {
            parse_go_int(&value).ok_or_else(invalid)?;
            probe = value;
        } else if let Some(flag) = STRING_FLAGS.iter().find(|f| **f == name) {
            strings.insert(flag, value);
        } else {
            return Err(invalid());
        }
    }
    if !rest.is_empty() {
        return Err("unexpected command arguments".into());
    }
    let get = |name: &str| strings.get(name).cloned().unwrap_or_default();
    let kind = get("lifecycle-kind");
    let unit = get("lifecycle-unit");
    let lifecycle_config = get("lifecycle-config");
    let authorized = bools.get("lifecycle-authorized").copied().unwrap_or(false);
    if !kind.is_empty() && kind != "systemd-user" {
        return Err("unsupported lifecycle kind".into());
    }
    if authorized && (kind != "systemd-user" || unit.is_empty()) {
        return Err("authorized lifecycle binding is incomplete".into());
    }
    if !authorized && (!kind.is_empty() || !unit.is_empty() || !lifecycle_config.is_empty()) {
        return Err("lifecycle binding requires explicit authority".into());
    }
    let probe_seconds = parse_go_int(&probe).ok_or_else(invalid)?;
    if !(1..=3600).contains(&probe_seconds) {
        return Err("probe interval must be between 1 and 3600 seconds".into());
    }
    let desired = strings
        .get("desired-service-state")
        .cloned()
        .unwrap_or_else(|| "enabled".into());
    if desired != "enabled" && desired != "disabled" {
        return Err("desired service state must be enabled or disabled".into());
    }
    if unit.contains(['\r', '\n', '\0']) {
        return Err("lifecycle unit is invalid".into());
    }
    Ok(Flags {
        discovery: Options {
            config_path: get("config"),
            endpoint: get("endpoint"),
            credential_file: get("credential-file"),
            tls_certificate: get("tls-certificate"),
            expected_device_id: get("expected-device-id"),
            insecure_tls: bools.get("insecure-tls").copied().unwrap_or(false),
        },
        binding: systemd::Binding {
            authorized,
            unit,
            config_path: lifecycle_config,
        },
        probe_seconds,
        desired_service_state: desired,
    })
}

/// `strconv.ParseBool`.
fn parse_go_bool(text: &str) -> Option<bool> {
    match text {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// `strconv.ParseInt(text, 0, 64)`: optional sign; `0x`, `0b`, `0o` or a
/// leading `0` (octal) select the base; underscores only between digits.
fn parse_go_int(text: &str) -> Option<i64> {
    let (negative, unsigned) = match text.as_bytes().first()? {
        b'+' => (false, &text[1..]),
        b'-' => (true, &text[1..]),
        _ => (false, text),
    };
    let bytes = unsigned.as_bytes();
    let (radix, body) = if bytes.len() >= 2 && bytes[0] == b'0' {
        match bytes[1].to_ascii_lowercase() {
            b'x' => (16, &unsigned[2..]),
            b'b' => (2, &unsigned[2..]),
            b'o' => (8, &unsigned[2..]),
            _ => (8, &unsigned[1..]),
        }
    } else {
        (10, unsigned)
    };
    if body.contains('_') && !underscore_ok(unsigned) {
        return None;
    }
    let digits: String = body.chars().filter(|&c| c != '_').collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let value = i128::from_str_radix(&digits, radix).ok()?;
    i64::try_from(if negative { -value } else { value }).ok()
}

/// Go's `underscoreOK`: an underscore only after a digit or base prefix
/// and before a digit.
fn underscore_ok(unsigned: &str) -> bool {
    let bytes = unsigned.as_bytes();
    let mut saw = b'^';
    let mut index = 0;
    let mut hex = false;
    if bytes.len() >= 2
        && bytes[0] == b'0'
        && matches!(bytes[1].to_ascii_lowercase(), b'b' | b'o' | b'x')
    {
        index = 2;
        saw = b'0';
        hex = bytes[1].eq_ignore_ascii_case(&b'x');
    }
    for &byte in &bytes[index..] {
        if byte.is_ascii_digit() || (hex && byte.is_ascii_hexdigit()) {
            saw = b'0';
        } else if byte == b'_' {
            if saw != b'0' {
                return false;
            }
            saw = b'_';
        } else {
            if saw == b'_' {
                return false;
            }
            saw = b'!';
        }
    }
    saw != b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(args: &[&str]) -> Result<Flags, String> {
        parse_flags(&args.iter().map(OsString::from).collect::<Vec<_>>())
    }

    #[test]
    fn the_plugin_invocation_parses() {
        let parsed = flags(&[
            "--probe-interval-seconds",
            "15",
            "--desired-service-state",
            "enabled",
            "--lifecycle-kind",
            "systemd-user",
            "--lifecycle-authorized",
            "--lifecycle-unit",
            "syncthing.service",
        ])
        .unwrap();
        assert_eq!(parsed.probe_seconds, 15);
        assert!(parsed.binding.authorized);
        assert_eq!(parsed.binding.unit, "syncthing.service");
    }

    #[test]
    fn go_flag_rules_and_syncshell_checks() {
        assert_eq!(
            flags(&["-probe-interval-seconds=0x10"])
                .unwrap()
                .probe_seconds,
            16
        );
        assert_eq!(
            flags(&["--probe-interval-seconds", "010"])
                .unwrap()
                .probe_seconds,
            8
        );
        assert!(
            flags(&["--json=false", "--insecure-tls=1"])
                .unwrap()
                .discovery
                .insecure_tls
        );
        assert_eq!(
            flags(&["--desired-service-state=disabled"])
                .unwrap()
                .desired_service_state,
            "disabled"
        );
        for (args, message) in [
            (&["--desktop-authorized"][..], "invalid command arguments"),
            (&["--probe-interval-seconds"], "invalid command arguments"),
            (
                &["--probe-interval-seconds", "x"],
                "invalid command arguments",
            ),
            (&["--json=yes"], "invalid command arguments"),
            (&["---x"], "invalid command arguments"),
            (&["extra"], "unexpected command arguments"),
            (&["--", "x"], "unexpected command arguments"),
            (&["-"], "unexpected command arguments"),
            (
                &["--lifecycle-kind", "launchd"],
                "unsupported lifecycle kind",
            ),
            (
                &["--lifecycle-authorized"],
                "authorized lifecycle binding is incomplete",
            ),
            (
                &["--lifecycle-unit", "x.service"],
                "lifecycle binding requires explicit authority",
            ),
            (
                &["--probe-interval-seconds", "3601"],
                "probe interval must be between 1 and 3600 seconds",
            ),
            (
                &["--desired-service-state", "on"],
                "desired service state must be enabled or disabled",
            ),
        ] {
            assert_eq!(flags(args).err().as_deref(), Some(message), "{args:?}");
        }
        assert!(flags(&["--",]).is_ok());
        assert!(flags(&["--lifecycle-authorized", "false"]).is_err());
    }

    #[test]
    fn integers_parse_like_strconv() {
        assert_eq!(parse_go_int("15"), Some(15));
        assert_eq!(parse_go_int("-0b101"), Some(-5));
        assert_eq!(parse_go_int("0o17"), Some(15));
        assert_eq!(parse_go_int("0x_1F"), Some(31));
        assert_eq!(parse_go_int("1_000"), Some(1000));
        assert_eq!(parse_go_int("1__0"), None);
        assert_eq!(parse_go_int("_1"), None);
        assert_eq!(parse_go_int("0x+5"), None);
        assert_eq!(parse_go_int("0x"), None);
        assert_eq!(parse_go_int("9223372036854775808"), None);
        assert_eq!(parse_go_int(""), None);
        assert_eq!(parse_go_int("09"), None);
        assert_eq!(parse_go_int("0"), Some(0));
    }
}
