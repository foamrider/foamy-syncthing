//! Bounded JSON lines, protocol v2 (Syncshell `internal/protocol`).
//!
//! Output frames: `hello` first, then `snapshot` (strictly increasing
//! revisions), `result` (one per accepted request), and finally `end`
//! (`shutdown` or `stdin`) or `fatal`. Every frame, in and out, is at most
//! 8 MiB including its newline. Requests are strict JSON objects with
//! exactly the fields `v` (2), `type`, `id` (1 to 128 bytes, no CR, LF or
//! NUL, not one of the last 4096 IDs) and, per type, `config`, `action` and
//! `args`; anything else is fatal.

use std::collections::{HashSet, VecDeque};
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::actions::Arguments;
use super::gocompat::to_json;
use super::session::{OperationalConfig, Session};
use super::types::{ActionResult, Error, Published, Snapshot};

pub const VERSION: i64 = 2;
pub const MAX_LINE_BYTES: usize = 8 << 20;
const MAX_REQUEST_ID: usize = 128;
const MAX_REMEMBERED_REQUESTS: usize = 4096;

enum Message {
    Line(Vec<u8>),
    TooLong,
    ReadError,
    Eof,
    Update,
}

#[derive(Serialize)]
struct Hello<'a> {
    v: i64,
    #[serde(rename = "type")]
    kind: &'a str,
    build: Build<'a>,
}

#[derive(Serialize)]
struct Build<'a> {
    version: &'a str,
}

#[derive(Serialize)]
struct SnapshotFrame<'a> {
    v: i64,
    #[serde(rename = "type")]
    kind: &'a str,
    revision: u64,
    state: &'a Snapshot,
}

#[derive(Serialize)]
struct ResultFrame<'a> {
    v: i64,
    #[serde(rename = "type")]
    kind: &'a str,
    id: &'a str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a Error>,
}

#[derive(Serialize)]
struct Fatal<'a> {
    v: i64,
    #[serde(rename = "type")]
    kind: &'a str,
    code: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct End<'a> {
    v: i64,
    #[serde(rename = "type")]
    kind: &'a str,
    reason: &'a str,
}

/// A request line; `null` reads as the zero value, as in Go.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default)]
    v: Option<i64>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    config: Option<Value>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    args: Option<Value>,
}

struct Output {
    stdout: io::Stdout,
    last_revision: u64,
}

impl Output {
    fn frame<T: Serialize>(&mut self, value: &T) -> Result<(), String> {
        let mut encoded = to_json(value).map_err(|e| format!("encode protocol frame: {e}"))?;
        if encoded.len() + 1 > MAX_LINE_BYTES {
            return Err("protocol output exceeds line bound".into());
        }
        encoded.push(b'\n');
        let mut stdout = self.stdout.lock();
        stdout
            .write_all(&encoded)
            .and_then(|()| stdout.flush())
            .map_err(|e| e.to_string())
    }

    fn snapshot(&mut self, published: &Published) -> Result<(), String> {
        self.frame(&SnapshotFrame {
            v: VERSION,
            kind: "snapshot",
            revision: published.revision,
            state: &published.state,
        })?;
        self.last_revision = published.revision;
        Ok(())
    }

    /// A newer snapshot, if there is one.
    fn newer(&mut self, published: &Published) -> Result<(), String> {
        if published.revision > self.last_revision {
            self.snapshot(published)
        } else {
            Ok(())
        }
    }

    fn result(&mut self, id: &str, result: &ActionResult) -> Result<(), String> {
        self.frame(&ResultFrame {
            v: VERSION,
            kind: "result",
            id,
            ok: result.ok,
            data: result.data.as_ref(),
            error: result.error.as_ref(),
        })
    }

    fn fatal(&mut self, code: &str, message: &str) {
        let _ = self.frame(&Fatal {
            v: VERSION,
            kind: "fatal",
            code,
            message,
        });
    }

    fn end(&mut self, reason: &str) -> Result<(), String> {
        self.frame(&End {
            v: VERSION,
            kind: "end",
            reason,
        })
    }
}

/// Reads `\n`-terminated lines (a trailing `\r` dropped, a last line
/// without newline kept), failing once a line can't fit the frame bound.
fn read_lines(sender: SyncSender<Message>) {
    let mut stdin = io::stdin().lock();
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 64 << 10];
    loop {
        let read = match stdin.read(&mut chunk) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                let _ = sender.send(Message::ReadError);
                return;
            }
        };
        if read == 0 {
            if !pending.is_empty() {
                strip_cr(&mut pending);
                if sender
                    .send(Message::Line(std::mem::take(&mut pending)))
                    .is_err()
                {
                    return;
                }
            }
            let _ = sender.send(Message::Eof);
            return;
        }
        let mut data = &chunk[..read];
        while let Some(newline) = data.iter().position(|&b| b == b'\n') {
            if pending.len() + newline + 1 > MAX_LINE_BYTES - 1 {
                let _ = sender.send(Message::TooLong);
                return;
            }
            pending.extend_from_slice(&data[..newline]);
            strip_cr(&mut pending);
            if sender
                .send(Message::Line(std::mem::take(&mut pending)))
                .is_err()
            {
                return;
            }
            data = &data[newline + 1..];
        }
        pending.extend_from_slice(data);
        if pending.len() >= MAX_LINE_BYTES - 1 {
            let _ = sender.send(Message::TooLong);
            return;
        }
    }
}

fn strip_cr(line: &mut Vec<u8>) {
    if line.last() == Some(&b'\r') {
        line.pop();
    }
}

struct RequestIds {
    seen: HashSet<String>,
    order: VecDeque<String>,
}

impl RequestIds {
    fn validate(&mut self, line: &[u8]) -> Result<Request, String> {
        // An object only: serde would also read a struct from an array.
        let malformed = || "request is malformed".to_string();
        let value: Value = serde_json::from_slice(line).map_err(|_| malformed())?;
        if !value.is_object() {
            return Err(malformed());
        }
        let request = Request::deserialize(value).map_err(|_| malformed())?;
        if request.v.unwrap_or(0) != VERSION {
            return Err("protocol major 2 required".into());
        }
        let id = request.id.as_deref().unwrap_or("");
        if id.is_empty() || id.len() > MAX_REQUEST_ID || id.contains(['\r', '\n', '\0']) {
            return Err("request id is invalid".into());
        }
        if self.seen.contains(id) {
            return Err("request id is duplicated".into());
        }
        if self.order.len() == MAX_REMEMBERED_REQUESTS
            && let Some(oldest) = self.order.pop_front()
        {
            self.seen.remove(&oldest);
        }
        self.seen.insert(id.to_string());
        self.order.push_back(id.to_string());
        Ok(request)
    }
}

/// The stream; `Err` is the message for standard error (exit status 1).
pub fn run(session: Arc<Session>, build: &str) -> Result<(), String> {
    let mut out = Output {
        stdout: io::stdout(),
        last_revision: 0,
    };
    out.frame(&Hello {
        v: VERSION,
        kind: "hello",
        build: Build { version: build },
    })?;
    let (initial, _) = session.refresh();
    out.snapshot(&initial)?;

    let (sender, receiver): (SyncSender<Message>, Receiver<Message>) = mpsc::sync_channel(2);
    let lines = sender.clone();
    thread::spawn(move || read_lines(lines));
    let latest: Arc<Mutex<Option<Published>>> = Arc::new(Mutex::new(None));
    {
        let session = Arc::clone(&session);
        let latest = Arc::clone(&latest);
        thread::spawn(move || {
            super::events::run(&session, &|published| {
                *latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(published);
                // A full queue is fine: the slot is checked after every message.
                if let Err(TrySendError::Disconnected(_)) = sender.try_send(Message::Update) {}
            });
        });
    }
    let mut ids = RequestIds {
        seen: HashSet::new(),
        order: VecDeque::new(),
    };
    loop {
        let message = receiver
            .recv()
            .map_err(|_| "protocol input ended".to_string())?;
        match message {
            Message::Update => {}
            Message::Eof => return out.end("stdin"),
            Message::TooLong => {
                out.fatal("line_too_large", "protocol input is invalid");
                return Err("bufio.Scanner: token too long".into());
            }
            Message::ReadError => {
                out.fatal("malformed_jsonl", "protocol input is invalid");
                return Err("protocol input could not be read".into());
            }
            Message::Line(line) => match handle(&session, &mut out, &mut ids, &line) {
                Ok(true) => return out.end("shutdown"),
                Ok(false) => {}
                Err(Failure::Protocol(message)) => {
                    out.fatal("protocol_request", &message);
                    return Err(message);
                }
                Err(Failure::Output(message)) => return Err(message),
            },
        }
        let update = latest.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(published) = update {
            out.newer(&published)?;
        }
    }
}

enum Failure {
    /// A bad request: a `fatal` frame, then exit.
    Protocol(String),
    /// Standard output failed.
    Output(String),
}

fn handle(
    session: &Session,
    out: &mut Output,
    ids: &mut RequestIds,
    line: &[u8],
) -> Result<bool, Failure> {
    let request = ids.validate(line).map_err(Failure::Protocol)?;
    let id = request.id.clone().unwrap_or_default();
    let output = Failure::Output;
    match request.kind.as_deref().unwrap_or("") {
        "configure" => {
            let config = match request.config {
                Some(Value::Object(object)) => {
                    OperationalConfig::deserialize(Value::Object(object)).ok()
                }
                _ => None,
            };
            let result = match config {
                Some(config) => session.configure(&config),
                None => {
                    ActionResult::rejected("invalid_config", "configuration request is invalid")
                }
            };
            out.newer(&session.current()).map_err(output)?;
            out.result(&id, &result).map_err(output)?;
        }
        "refresh" => {
            let (published, error) = session.refresh();
            out.newer(&session.current()).map_err(output)?;
            let result = match error {
                None => ActionResult::ok(),
                Some(_) => ActionResult {
                    error: published.state.connection.error.clone(),
                    ..ActionResult::default()
                },
            };
            out.result(&id, &result).map_err(output)?;
        }
        "action" => {
            let arguments = match request.args {
                Some(Value::Object(object)) => Arguments::deserialize(Value::Object(object)).ok(),
                _ => None,
            };
            let result = match arguments {
                Some(arguments) => session.act(request.action.as_deref().unwrap_or(""), &arguments),
                None => ActionResult::rejected("invalid_action", "action arguments are invalid"),
            };
            out.newer(&session.current()).map_err(output)?;
            out.result(&id, &result).map_err(output)?;
        }
        "shutdown" => {
            out.result(&id, &ActionResult::ok()).map_err(output)?;
            return Ok(true);
        }
        _ => return Err(Failure::Protocol("request type is unsupported".into())),
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> RequestIds {
        RequestIds {
            seen: HashSet::new(),
            order: VecDeque::new(),
        }
    }

    #[test]
    fn requests_are_strict() {
        let mut ids = ids();
        assert!(
            ids.validate(br#"{"v":2,"type":"refresh","id":"1"}"#)
                .is_ok()
        );
        for (line, message) in [
            (
                &br#"{"v":2,"type":"refresh","id":"1"}"#[..],
                "request id is duplicated",
            ),
            (
                br#"{"v":2,"type":"refresh","id":"2","x":1}"#,
                "request is malformed",
            ),
            (
                br#"{"v":2.0,"type":"refresh","id":"3"}"#,
                "request is malformed",
            ),
            (
                br#"{"v":null,"type":"refresh","id":"4"}"#,
                "protocol major 2 required",
            ),
            (
                br#"{"type":"refresh","id":"5"}"#,
                "protocol major 2 required",
            ),
            (
                br#"{"v":2,"type":"refresh","id":5}"#,
                "request is malformed",
            ),
            (br#"{"v":2,"type":"refresh"}"#, "request id is invalid"),
            (
                br#"{"v":2,"type":"refresh","id":"a\u0000"}"#,
                "request id is invalid",
            ),
            (b"[]", "request is malformed"),
            (b"", "request is malformed"),
        ] {
            assert_eq!(
                ids.validate(line).err().as_deref(),
                Some(message),
                "{}",
                String::from_utf8_lossy(line)
            );
        }
    }

    #[test]
    fn the_id_window_forgets_the_oldest() {
        let mut ids = ids();
        for index in 0..MAX_REMEMBERED_REQUESTS + 1 {
            ids.validate(format!(r#"{{"v":2,"type":"refresh","id":"{index}"}}"#).as_bytes())
                .unwrap();
        }
        assert!(
            ids.validate(br#"{"v":2,"type":"refresh","id":"0"}"#)
                .is_ok()
        );
        assert!(
            ids.validate(br#"{"v":2,"type":"refresh","id":"2"}"#)
                .is_err()
        );
    }

    #[test]
    fn frames_are_go_compatible() {
        let error = Error::new("x", "a<b");
        let frame = ResultFrame {
            v: 2,
            kind: "result",
            id: "1",
            ok: false,
            data: None,
            error: Some(&error),
        };
        assert_eq!(
            String::from_utf8(to_json(&frame).unwrap()).unwrap(),
            r#"{"v":2,"type":"result","id":"1","ok":false,"error":{"code":"x","message":"a\u003cb"}}"#
        );
    }
}
