//! `pick-folder [title [accept label]]`: the desktop's folder chooser,
//! through xdg-desktop-portal's `org.freedesktop.portal.FileChooser.OpenFile`
//! (`directory: true`) on the session bus.
//!
//! The call: no parent window, the title (default "Choose a folder"),
//! `modal`, one folder, and the accept button's label when one is given
//! (the portal's own otherwise). An empty argument counts as none. The bus
//! is `$DBUS_SESSION_BUS_ADDRESS`, else `$XDG_RUNTIME_DIR/bus`, else
//! `/run/user/<uid>/bus`. The helper waits up to 30 minutes for the choice
//! and dies with its parent. It starts no programs and reads no files.
//!
//! Replies (one JSON line on stdout): `{"ok": true, "path": "/absolute/path"}`;
//! `{"ok": true, "cancelled": true}` when the dialog was closed without a
//! choice; or `{"ok": false, "code": …, "error": …}` and exit 1 with
//! `INVALID_ARGUMENT` (more than two arguments, or one that isn't UTF-8),
//! `NO_SESSION_BUS` (no bus, or it refused us), `PORTAL_UNAVAILABLE` (no
//! portal, or one without a file chooser) or `PORTAL_FAILED` (an error from
//! the portal, a choice that isn't a local path or isn't UTF-8, the time
//! limit).
//!
//! Adapted from foamy-lock's `native/src/studio/portal.rs` (`studio
//! pick-picture`); the same file in foamy-github and foamy-syncthing.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use foamy_helper::Failure;
use serde_json::{Value, json};

use crate::dbus::{self, Address, Connection, Error, Message, Writer};

pub const DEFAULT_TITLE: &str = "Choose a folder";
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REQUEST: &str = "org.freedesktop.portal.Request";
const WAIT: Duration = Duration::from_secs(30 * 60);

/// Where the session bus is: `$DBUS_SESSION_BUS_ADDRESS` and
/// `$XDG_RUNTIME_DIR` (absolute).
#[derive(Debug, Default, Clone)]
pub struct BusEnv {
    pub bus: Option<String>,
    pub runtime: Option<PathBuf>,
}

impl BusEnv {
    pub fn current() -> BusEnv {
        BusEnv {
            bus: std::env::var_os("DBUS_SESSION_BUS_ADDRESS")
                .and_then(|value| value.into_string().ok())
                .filter(|value| !value.is_empty()),
            runtime: std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute()),
        }
    }
}

/// The session bus.
pub fn bus_address(env: &BusEnv) -> Result<Address, Failure> {
    if let Some(list) = &env.bus {
        return dbus::parse_address(list).ok_or_else(|| {
            Failure::coded(
                "NO_SESSION_BUS",
                format!("DBUS_SESSION_BUS_ADDRESS has no unix socket: {list}"),
            )
        });
    }
    let runtime = env
        .runtime
        .clone()
        .unwrap_or_else(|| format!("/run/user/{}", rustix::process::getuid().as_raw()).into());
    Ok(Address::Path(runtime.join("bus")))
}

/// `OpenFile`'s body (`ssa{sv}`) for one folder.
pub fn open_folder_body(title: &str, accept: Option<&str>, token: &str) -> Vec<u8> {
    let mut w = Writer::new();
    w.string("");
    w.string(title);
    w.array(8, |w| {
        w.entry("handle_token", "s", |w| w.string(token));
        if let Some(label) = accept {
            w.entry("accept_label", "s", |w| w.string(label));
        }
        w.entry("modal", "b", |w| w.boolean(true));
        w.entry("multiple", "b", |w| w.boolean(false));
        w.entry("directory", "b", |w| w.boolean(true));
    });
    w.buf
}

/// The `Response` signal's body (`ua{sv}`): the response code and the
/// chosen URIs.
pub fn parse_response(message: &Message) -> Result<(u32, Vec<String>), Error> {
    if message.signature != "ua{sv}" {
        return Err(Error::Protocol(format!(
            "a Response of type {:?}",
            message.signature
        )));
    }
    let mut r = message.reader();
    let code = r.u32()?;
    let mut uris = Vec::new();
    r.array(8, |r| {
        r.align(8)?;
        let key = r.string()?;
        let signature = r.signature()?;
        if key == "uris" && signature == "as" {
            r.array(4, |r| {
                uris.push(r.string()?.to_string());
                Ok(())
            })
        } else {
            r.skip(signature)
        }
    })?;
    Ok((code, uris))
}

/// A local path from a `file:` URI (empty or `localhost` host). A folder's
/// URI may end in `/`; the path doesn't (except `/` itself).
pub fn uri_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') || path.contains(['?', '#']) {
        return None;
    }
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    if out.contains(&0) {
        return None;
    }
    while out.len() > 1 && out.ends_with(b"/") {
        out.pop();
    }
    String::from_utf8(out).ok()
}

/// A handle token: `prefix` and 8 random characters of `[a-z0-9]`.
pub fn handle_token(prefix: &str) -> std::io::Result<String> {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut bytes = [0u8; 8];
    let mut filled = 0;
    while filled < bytes.len() {
        filled +=
            rustix::rand::getrandom(&mut bytes[filled..], rustix::rand::GetRandomFlags::empty())?;
    }
    let suffix: String = bytes
        .iter()
        .map(|&b| CHARS[b as usize % CHARS.len()] as char)
        .collect();
    Ok(format!("{prefix}{suffix}"))
}

fn failed(error: Error) -> Failure {
    match error {
        Error::NoBus(text) => Failure::coded("NO_SESSION_BUS", text),
        Error::Timeout => Failure::coded("PORTAL_FAILED", "no choice within the time limit"),
        other => Failure::coded("PORTAL_FAILED", other.to_string()),
    }
}

const UNAVAILABLE: [&str; 5] = [
    "org.freedesktop.DBus.Error.ServiceUnknown",
    "org.freedesktop.DBus.Error.NameHasNoOwner",
    "org.freedesktop.DBus.Error.UnknownMethod",
    "org.freedesktop.DBus.Error.UnknownInterface",
    "org.freedesktop.DBus.Error.UnknownObject",
];

/// The portal conversation on `address`, with the handle token `token`.
pub fn pick_on(
    address: &Address,
    title: &str,
    accept: Option<&str>,
    token: &str,
    deadline: Instant,
) -> Result<Value, Failure> {
    let mut bus = Connection::open(address, deadline).map_err(failed)?;
    let sender = bus.unique_name.trim_start_matches(':').replace('.', "_");
    // Subscribe to the predicted request path before the call, so the
    // Response can't come first.
    let mut handles = vec![format!("{PORTAL_PATH}/request/{sender}/{token}")];
    let add_match = |bus: &mut Connection, handle: &str| {
        let rule = format!("type='signal',interface='{REQUEST}',member='Response',path='{handle}'");
        let mut body = Writer::new();
        body.string(&rule);
        bus.send(
            Message::call(
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "AddMatch",
            )
            .with_body("s", body.buf),
        )
    };
    add_match(&mut bus, &handles[0]).map_err(failed)?;
    let call = bus
        .send(
            Message::call(
                PORTAL,
                PORTAL_PATH,
                "org.freedesktop.portal.FileChooser",
                "OpenFile",
            )
            .with_body("ssa{sv}", open_folder_body(title, accept, token)),
        )
        .map_err(failed)?;
    // The portal's unique name, from its reply to the call: only a Response
    // it sent counts (any client can emit a signal on any path). Responses
    // that come before the reply wait for it (a few at most).
    let mut portal: Option<String> = None;
    let mut early: Vec<Message> = Vec::new();
    loop {
        let message = bus.read().map_err(failed)?;
        match message.kind {
            dbus::METHOD_RETURN if message.reply_serial == Some(call) => {
                let Some(name) = message.sender.clone().filter(|name| name.starts_with(':')) else {
                    return Err(Failure::coded(
                        "PORTAL_FAILED",
                        "the portal's reply has no unique sender",
                    ));
                };
                // An older portal may pick its own path: follow that one too.
                if let Ok(handle) = message.reader().string()
                    && !handles.iter().any(|known| known == handle)
                {
                    let handle = handle.to_string();
                    add_match(&mut bus, &handle).map_err(failed)?;
                    handles.push(handle);
                }
                for response in std::mem::take(&mut early) {
                    if is_response(&response, &handles, &name) {
                        return answer(&response);
                    }
                }
                portal = Some(name);
            }
            dbus::ERROR if message.reply_serial == Some(call) => {
                let name = message.error_name.clone().unwrap_or_default();
                let text = message.first_string().unwrap_or_default();
                let code = if UNAVAILABLE.contains(&name.as_str()) {
                    "PORTAL_UNAVAILABLE"
                } else {
                    "PORTAL_FAILED"
                };
                return Err(Failure::coded(code, format!("{name}: {text}")));
            }
            dbus::SIGNAL => match &portal {
                Some(name) if is_response(&message, &handles, name) => return answer(&message),
                None if is_response(&message, &handles, "") && early.len() < MAX_EARLY => {
                    early.push(message);
                }
                _ => {}
            },
            _ => {}
        }
    }
}

/// Responses kept until the portal's reply says who the portal is.
const MAX_EARLY: usize = 8;

/// A `Response` on one of `handles`, from `portal` (any sender when
/// `portal` is empty: not known yet).
fn is_response(message: &Message, handles: &[String], portal: &str) -> bool {
    message.kind == dbus::SIGNAL
        && message.interface.as_deref() == Some(REQUEST)
        && message.member.as_deref() == Some("Response")
        && message
            .path
            .as_ref()
            .is_some_and(|path| handles.contains(path))
        && (portal.is_empty() || message.sender.as_deref() == Some(portal))
}

/// The reply for the portal's `Response`.
fn answer(message: &Message) -> Result<Value, Failure> {
    let (code, uris) = parse_response(message).map_err(failed)?;
    match code {
        0 => {
            let uri = uris
                .first()
                .ok_or_else(|| Failure::coded("PORTAL_FAILED", "the portal chose nothing"))?;
            let path = uri_path(uri).ok_or_else(|| {
                Failure::coded("PORTAL_FAILED", format!("not a local path: {uri}"))
            })?;
            Ok(json!({"ok": true, "path": path}))
        }
        1 | 2 => Ok(json!({"ok": true, "cancelled": true})),
        other => Err(Failure::coded(
            "PORTAL_FAILED",
            format!("the portal answered {other}"),
        )),
    }
}

/// The title and accept label from `pick-folder`'s arguments.
pub fn arguments<'a>(
    helper: &str,
    args: &'a [OsString],
) -> Result<(&'a str, Option<&'a str>), Failure> {
    let usage = || {
        Failure::coded(
            "INVALID_ARGUMENT",
            format!("usage: {helper} pick-folder [title [accept label]]"),
        )
    };
    let words: Option<Vec<&str>> = args.iter().map(|arg| arg.to_str()).collect();
    let words = words.ok_or_else(usage)?;
    let (title, accept) = match words.as_slice() {
        [] => ("", ""),
        [title] => (*title, ""),
        [title, accept] => (*title, *accept),
        _ => return Err(usage()),
    };
    let title = if title.is_empty() {
        DEFAULT_TITLE
    } else {
        title
    };
    Ok((title, Some(accept).filter(|label| !label.is_empty())))
}

/// `<helper> pick-folder [title [accept label]]`; `helper` is the binary's
/// name (`foamy-github-helper`), which also names the handle token.
pub fn main(helper: &str, args: &[OsString]) -> Result<Value, Failure> {
    let (title, accept) = arguments(helper, args)?;
    // A panel that goes away takes its question with it.
    let _ = foamy_helper::process::die_with_parent();
    let prefix = format!("{}_", helper.trim_end_matches("-helper").replace('-', "_"));
    let token = handle_token(&prefix).map_err(|e| failed(Error::Io(e)))?;
    pick_on(
        &bus_address(&BusEnv::current())?,
        title,
        accept,
        &token,
        Instant::now() + WAIT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dbus::{Reader, Wire};
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::thread;

    /// A fresh private directory under the system temp dir, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            use std::os::unix::fs::PermissionsExt;
            static COUNT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "foamy-portal-{tag}-{}-{}",
                std::process::id(),
                COUNT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            Scratch(path)
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The `{sv}` options of an `OpenFile` body: (key, signature, value as
    /// text for `s`, `b` and `u`), after the parent window and the title.
    fn options_of(body: Vec<u8>) -> (String, Vec<(String, String, String)>) {
        let message = Message::default().with_body("ssa{sv}", body);
        let mut r: Reader = message.reader();
        assert_eq!(r.string().unwrap(), "");
        let title = r.string().unwrap().to_string();
        let mut options = Vec::new();
        r.array(8, |r| {
            r.align(8)?;
            let key = r.string()?.to_string();
            let signature = r.signature()?.to_string();
            let value = match signature.as_str() {
                "s" => r.string()?.to_string(),
                "b" | "u" => r.u32()?.to_string(),
                _ => {
                    r.skip(&signature)?;
                    "…".into()
                }
            };
            options.push((key, signature, value));
            Ok(())
        })
        .unwrap();
        (title, options)
    }

    fn option(key: &str, signature: &str, value: &str) -> (String, String, String) {
        (key.into(), signature.into(), value.into())
    }

    #[test]
    fn the_call_asks_for_one_folder() {
        assert_eq!(
            options_of(open_folder_body("Velg en mappe", Some("Velg"), "t1")),
            (
                "Velg en mappe".into(),
                vec![
                    option("handle_token", "s", "t1"),
                    option("accept_label", "s", "Velg"),
                    option("modal", "b", "1"),
                    option("multiple", "b", "0"),
                    option("directory", "b", "1"),
                ]
            )
        );
        // Without a label the portal picks its own: no accept_label at all.
        let (_, options) = options_of(open_folder_body("T", None, "t2"));
        assert!(options.iter().all(|(key, _, _)| key != "accept_label"));
        assert!(options.contains(&option("directory", "b", "1")));
    }

    #[test]
    fn arguments_give_the_title_and_label() {
        let args = |words: &[&str]| -> Vec<OsString> { words.iter().map(OsString::from).collect() };
        let helper = "foamy-x-helper";
        assert_eq!(
            arguments(helper, &args(&[])).unwrap(),
            (DEFAULT_TITLE, None)
        );
        assert_eq!(arguments(helper, &args(&["Velg"])).unwrap(), ("Velg", None));
        assert_eq!(
            arguments(helper, &args(&["Velg en mappe", "Velg"])).unwrap(),
            ("Velg en mappe", Some("Velg"))
        );
        assert_eq!(
            arguments(helper, &args(&["", ""])).unwrap(),
            (DEFAULT_TITLE, None)
        );
        let failure = arguments(helper, &args(&["a", "b", "c"])).unwrap_err();
        assert_eq!(failure.code, "INVALID_ARGUMENT");
        assert!(failure.message.contains("foamy-x-helper pick-folder"));
        use std::os::unix::ffi::OsStringExt;
        let bad = vec![OsString::from_vec(vec![0xff])];
        assert_eq!(
            arguments(helper, &bad).unwrap_err().code,
            "INVALID_ARGUMENT"
        );
    }

    #[test]
    fn tokens_are_valid_object_path_elements() {
        let token = handle_token("foamy_x_").unwrap();
        assert!(
            token.starts_with("foamy_x_") && token.len() == 16,
            "{token}"
        );
        assert!(
            token
                .bytes()
                .all(|b| b == b'_' || b.is_ascii_lowercase() || b.is_ascii_digit())
        );
        assert_ne!(token, handle_token("foamy_x_").unwrap());
    }

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            uri_path("file:///home/me/src").as_deref(),
            Some("/home/me/src")
        );
        assert_eq!(
            uri_path("file:///home/me/src/").as_deref(),
            Some("/home/me/src")
        );
        assert_eq!(uri_path("file:///").as_deref(), Some("/"));
        assert_eq!(
            uri_path("file://localhost/home/me/Mapper%20%C3%A5").as_deref(),
            Some("/home/me/Mapper å")
        );
        for bad in [
            "https://x/a",
            "file://host/a",
            "file:relative",
            "file:///a%00b",
            "file:///a%zz",
            "file:///a%ff",
            "file:///a?b",
            "file:///a#b",
        ] {
            assert_eq!(uri_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_bus_comes_from_the_environment() {
        let mut env = BusEnv {
            bus: None,
            runtime: Some("/run/test".into()),
        };
        assert_eq!(
            bus_address(&env).unwrap(),
            Address::Path("/run/test/bus".into())
        );
        env.bus = Some("unix:path=/x/bus".into());
        assert_eq!(bus_address(&env).unwrap(), Address::Path("/x/bus".into()));
        env.bus = Some("tcp:host=localhost".into());
        assert_eq!(bus_address(&env).unwrap_err().code, "NO_SESSION_BUS");
        let fallback = BusEnv::default();
        assert_eq!(
            bus_address(&fallback).unwrap(),
            Address::Path(format!("/run/user/{}/bus", rustix::process::getuid().as_raw()).into())
        );
    }

    /// What the fake portal does with `OpenFile`.
    #[derive(Clone, Copy)]
    enum Portal {
        Choose,
        Cancel,
        OwnPath,
        Early,
        Missing,
        Refuse,
    }

    fn reply_to(call: &Message, kind: u8) -> Message {
        Message {
            kind,
            reply_serial: Some(call.serial),
            destination: call.sender.clone(),
            sender: Some("org.freedesktop.DBus".into()),
            ..Message::default()
        }
    }

    /// What `OpenFile` got: the title, its options and the AddMatch rules.
    type Seen = (String, Vec<(String, String, String)>, Vec<String>);

    /// A one-client bus with a portal behind it.
    fn serve(listener: UnixListener, portal: Portal) -> thread::JoinHandle<Seen> {
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut wire = Wire::new(stream, Instant::now() + Duration::from_secs(10));
            assert_eq!(wire.read_exact(1).unwrap(), b"\0");
            let auth = wire.read_line().unwrap();
            let uid = rustix::process::getuid().as_raw().to_string();
            let expected: String = uid.bytes().map(|b| format!("{b:02x}")).collect();
            assert_eq!(auth, format!("AUTH EXTERNAL {expected}"));
            wire.write(b"OK 0123456789abcdef0123456789abcdef\r\n")
                .unwrap();
            assert_eq!(wire.read_line().unwrap(), "BEGIN");
            let mut serial = 100;
            let mut send = |wire: &mut Wire, mut message: Message| {
                serial += 1;
                message.serial = serial;
                wire.write(&message.encode()).unwrap();
            };
            let mut matches = Vec::new();
            loop {
                let mut call = wire.read_message().unwrap();
                call.sender = Some(":1.42".into());
                match call.member.as_deref().unwrap_or("") {
                    "Hello" => {
                        let mut body = Writer::new();
                        body.string(":1.42");
                        // A signal first, as a real bus sends NameAcquired.
                        let acquired = Message {
                            kind: dbus::SIGNAL,
                            path: Some("/org/freedesktop/DBus".into()),
                            interface: Some("org.freedesktop.DBus".into()),
                            member: Some("NameAcquired".into()),
                            ..Message::default()
                        }
                        .with_body("s", body.buf.clone());
                        send(&mut wire, acquired);
                        send(
                            &mut wire,
                            reply_to(&call, dbus::METHOD_RETURN).with_body("s", body.buf),
                        );
                    }
                    "AddMatch" => {
                        matches.push(call.reader().string().unwrap().to_string());
                        send(&mut wire, reply_to(&call, dbus::METHOD_RETURN));
                    }
                    "OpenFile" => {
                        assert_eq!(call.destination.as_deref(), Some(PORTAL));
                        assert_eq!(
                            call.interface.as_deref(),
                            Some("org.freedesktop.portal.FileChooser")
                        );
                        assert_eq!(call.signature, "ssa{sv}");
                        let (title, options) = options_of(call.body.clone());
                        let token = options
                            .iter()
                            .find(|(key, _, _)| key == "handle_token")
                            .map(|(_, _, value)| value.clone())
                            .unwrap_or_default();
                        let refuse = |name: &str, text: &str| {
                            let mut body = Writer::new();
                            body.string(text);
                            let mut error = reply_to(&call, dbus::ERROR).with_body("s", body.buf);
                            error.error_name = Some(name.into());
                            error
                        };
                        match portal {
                            Portal::Missing => {
                                send(
                                    &mut wire,
                                    refuse(
                                        "org.freedesktop.DBus.Error.ServiceUnknown",
                                        "The name is not activatable",
                                    ),
                                );
                                return (title, options, matches);
                            }
                            Portal::Refuse => {
                                send(
                                    &mut wire,
                                    refuse("org.freedesktop.portal.Error.Failed", "No, thanks"),
                                );
                                return (title, options, matches);
                            }
                            _ => {}
                        }
                        let path = match portal {
                            Portal::OwnPath => {
                                "/org/freedesktop/portal/desktop/request/1_42/other".to_string()
                            }
                            _ => format!("/org/freedesktop/portal/desktop/request/1_42/{token}"),
                        };
                        let response_with = |uri: &str, sender: &str| {
                            let mut body = Writer::new();
                            body.u32(if let Portal::Cancel = portal { 1 } else { 0 });
                            body.array(8, |w| {
                                w.entry("choices", "a(ss)", |w| w.array(8, |_| {}));
                                w.entry("uris", "as", |w| w.array(4, |w| w.string(uri)));
                                w.entry("writable", "b", |w| w.boolean(false));
                            });
                            Message {
                                kind: dbus::SIGNAL,
                                path: Some(path.clone()),
                                interface: Some(REQUEST.into()),
                                member: Some("Response".into()),
                                sender: Some(sender.into()),
                                ..Message::default()
                            }
                            .with_body("ua{sv}", body.buf)
                        };
                        let response = response_with("file:///home/me/Code/Fjord%20lys/", ":1.7");
                        // Another client's Response on the same path, sent
                        // first: not the portal's, so ignored.
                        let forged = response_with("file:///tmp/forged", ":1.66");
                        let mut body = Writer::new();
                        body.string(&path);
                        // The portal (":1.7") answers the call.
                        let mut reply =
                            reply_to(&call, dbus::METHOD_RETURN).with_body("o", body.buf);
                        reply.sender = Some(":1.7".into());
                        if let Portal::Early = portal {
                            // Its Response before its reply (and a forged one).
                            send(&mut wire, forged);
                            send(&mut wire, response);
                            send(&mut wire, reply);
                            return (title, options, matches);
                        }
                        send(&mut wire, reply);
                        if let Portal::OwnPath = portal {
                            // Wait for the client to subscribe to it.
                            let call = wire.read_message().unwrap();
                            matches.push(call.reader().string().unwrap().to_string());
                            send(&mut wire, reply_to(&call, dbus::METHOD_RETURN));
                        }
                        // A Response for someone else's request first: ignored.
                        let mut stray = response.clone();
                        stray.path = Some("/org/freedesktop/portal/desktop/request/1_9/x".into());
                        send(&mut wire, stray);
                        send(&mut wire, forged);
                        send(&mut wire, response);
                        return (title, options, matches);
                    }
                    other => panic!("unexpected call {other}"),
                }
            }
        })
    }

    fn fake_bus(tag: &str, portal: Portal, accept: Option<&str>) -> (Result<Value, Failure>, Seen) {
        let scratch = Scratch::new(tag);
        let socket = scratch.path("bus");
        let server = serve(UnixListener::bind(&socket).unwrap(), portal);
        let result = pick_on(
            &Address::Path(socket),
            "Velg en mappe",
            accept,
            "foamy_x_abc12345",
            Instant::now() + Duration::from_secs(10),
        );
        (result, server.join().unwrap())
    }

    #[test]
    fn a_choice_comes_back_as_a_path() {
        let (result, (title, options, matches)) = fake_bus("choose", Portal::Choose, Some("Velg"));
        assert_eq!(
            result.unwrap(),
            json!({"ok": true, "path": "/home/me/Code/Fjord lys"})
        );
        assert_eq!(title, "Velg en mappe");
        assert!(options.contains(&option("accept_label", "s", "Velg")));
        assert!(options.contains(&option("directory", "b", "1")));
        assert_eq!(
            matches,
            [
                "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='/org/freedesktop/portal/desktop/request/1_42/foamy_x_abc12345'"
            ]
        );
    }

    #[test]
    fn only_the_portals_response_counts_even_before_its_reply() {
        // Every fake run sends a forged Response (another sender, the right
        // path, file:///tmp/forged) before the portal's: Choose and OwnPath
        // get the real path. Here both come before the reply that names the
        // portal, and wait for it.
        let (result, _) = fake_bus("early", Portal::Early, None);
        assert_eq!(
            result.unwrap(),
            json!({"ok": true, "path": "/home/me/Code/Fjord lys"})
        );
    }

    #[test]
    fn a_closed_dialog_is_cancelled() {
        let (result, _) = fake_bus("cancel", Portal::Cancel, None);
        assert_eq!(result.unwrap(), json!({"ok": true, "cancelled": true}));
    }

    #[test]
    fn a_portal_with_its_own_path_is_followed() {
        let (result, (_, _, matches)) = fake_bus("ownpath", Portal::OwnPath, None);
        assert_eq!(result.unwrap()["path"], "/home/me/Code/Fjord lys");
        assert_eq!(matches.len(), 2);
        assert!(matches[1].ends_with("path='/org/freedesktop/portal/desktop/request/1_42/other'"));
    }

    #[test]
    fn a_missing_portal_is_unavailable_and_an_error_failed() {
        let (result, _) = fake_bus("missing", Portal::Missing, None);
        let failure = result.unwrap_err();
        assert_eq!(failure.code, "PORTAL_UNAVAILABLE");
        assert!(failure.message.contains("not activatable"), "{failure}");
        let (result, _) = fake_bus("refuse", Portal::Refuse, None);
        let failure = result.unwrap_err();
        assert_eq!(failure.code, "PORTAL_FAILED");
        assert!(failure.message.contains("No, thanks"), "{failure}");
    }

    #[test]
    fn no_bus_and_a_silent_bus_are_reported() {
        let scratch = Scratch::new("nobus");
        let missing = Address::Path(scratch.path("nothing"));
        let failure = pick_on(
            &missing,
            DEFAULT_TITLE,
            None,
            "t",
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap_err();
        assert_eq!(failure.code, "NO_SESSION_BUS");
        // A bus that accepts and never answers runs into the deadline.
        let listener = UnixListener::bind(scratch.path("silent")).unwrap();
        let started = Instant::now();
        let failure = pick_on(
            &Address::Path(scratch.path("silent")),
            DEFAULT_TITLE,
            None,
            "t",
            Instant::now() + Duration::from_millis(200),
        )
        .unwrap_err();
        assert_eq!(failure.code, "PORTAL_FAILED");
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(listener);
    }

    fn response(code: u32, uris: &[&str]) -> Message {
        let mut body = Writer::new();
        body.u32(code);
        body.array(8, |w| {
            w.entry("current_filter", "(sa(us))", |w| {
                w.structure(|w| {
                    w.string("All");
                    w.array(8, |w| {
                        w.structure(|w| {
                            w.u32(0);
                            w.string("*");
                        })
                    });
                })
            });
            w.entry("uris", "as", |w| {
                w.array(4, |w| {
                    for uri in uris {
                        w.string(uri);
                    }
                })
            });
        });
        Message {
            kind: dbus::SIGNAL,
            serial: 1,
            ..Message::default()
        }
        .with_body("ua{sv}", body.buf)
    }

    #[test]
    fn responses_parse_whatever_else_they_carry() {
        assert_eq!(
            parse_response(&response(0, &["file:///a", "file:///b"])).unwrap(),
            (0, vec!["file:///a".into(), "file:///b".into()])
        );
        let wrong = Message {
            kind: dbus::SIGNAL,
            serial: 1,
            ..Message::default()
        }
        .with_body("u", vec![0; 4]);
        assert!(parse_response(&wrong).is_err());
    }

    #[test]
    fn responses_become_replies() {
        assert_eq!(
            answer(&response(0, &["file:///srv/a%20b/"])).unwrap(),
            json!({"ok": true, "path": "/srv/a b"})
        );
        assert_eq!(
            answer(&response(2, &[])).unwrap(),
            json!({"ok": true, "cancelled": true})
        );
        for (code, uris) in [
            (0, vec![]),
            (0, vec!["https://example.com/x"]),
            (3, vec!["file:///a"]),
        ] {
            assert_eq!(
                answer(&response(code, &uris)).unwrap_err().code,
                "PORTAL_FAILED",
                "{code} {uris:?}"
            );
        }
    }
}
