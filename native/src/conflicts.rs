//! `conflicts`: which configured folder roots hold Syncthing conflict files.
//! Replaces `scripts/syncthing-conflicts.py` with the same contract (see the
//! crate documentation).

use std::ffi::{OsStr, OsString};
use std::fs::{self, ReadDir};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde_json::Value;

/// Largest accepted request.
pub const MAX_INPUT_BYTES: u64 = 1 << 20;
/// The scan gives up on its own after this long (the panel also kills it at 30 s).
const DEADLINE_SECONDS: u64 = 25;
/// Test-only: a shorter deadline in whole seconds (1 to 25).
const DEADLINE_ENV: &str = "FOAMY_SYNCTHING_CONFLICTS_DEADLINE";
/// The only diagnostic: filesystem errors name private paths.
const GENERIC: &str = "Could not check for Syncthing conflicts.";
/// Syncthing's own bookkeeping, not user data: versioned copies of resolved
/// conflicts keep the `.sync-conflict-` stem, so they would match forever.
const SKIPPED_DIRECTORIES: [&[u8]; 2] = [b".stversions", b".stfolder"];
/// `fnmatch("*.sync-conflict-*")`: the name contains this.
const PATTERN: &[u8] = b".sync-conflict-";

/// Set by whichever finishes first: the scan or the deadline.
static FINISHED: AtomicBool = AtomicBool::new(false);

pub fn main(args: &[OsString]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("Conflict helper accepts stdin only.");
        return ExitCode::from(2);
    }
    let deadline = std::env::var(DEADLINE_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| (1..=DEADLINE_SECONDS).contains(seconds))
        .unwrap_or(DEADLINE_SECONDS);
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(deadline));
        if !FINISHED.swap(true, Ordering::SeqCst) {
            eprintln!("{GENERIC}");
            std::process::exit(1);
        }
    });
    let result: io::Result<Vec<bool>> =
        read_paths().and_then(|paths| paths.iter().map(|p| has_conflict(p)).collect());
    if FINISHED.swap(true, Ordering::SeqCst) {
        // The deadline won and is exiting.
        loop {
            thread::park();
        }
    }
    match result {
        Ok(flags) => {
            // Index-aligned booleans only: no paths leave the helper. The
            // spacing is Python's json.dumps, which the panel never relied on.
            let text: Vec<&str> = flags
                .iter()
                .map(|&flag| if flag { "true" } else { "false" })
                .collect();
            let mut stdout = io::stdout().lock();
            match writeln!(stdout, "[{}]", text.join(", ")).and_then(|()| stdout.flush()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::from(1),
            }
        }
        Err(_) => {
            eprintln!("{GENERIC}");
            ExitCode::from(1)
        }
    }
}

/// The request: a JSON list of non-empty strings without NUL, at most 1 MiB.
fn read_paths() -> io::Result<Vec<OsString>> {
    let raw = foamy_helper::io::read_stdin_limited(MAX_INPUT_BYTES)?;
    parse_paths(&raw).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "request"))
}

pub fn parse_paths(raw: &[u8]) -> Option<Vec<OsString>> {
    let Value::Array(items) = serde_json::from_slice::<Value>(raw).ok()? else {
        return None;
    };
    items
        .into_iter()
        .map(|item| match item {
            Value::String(path) if !path.is_empty() && !path.contains('\0') => Some(path.into()),
            _ => None,
        })
        .collect()
}

/// Depth first over `root`, following `root` itself if it is a link but no
/// link inside it (like `find -H`), streaming each directory so a wide one is
/// never buffered. A missing root or one that is not a directory is clear;
/// an entry that vanishes during the walk is skipped; any other error fails
/// the whole scan.
pub fn has_conflict(root: &OsStr) -> io::Result<bool> {
    let root = Path::new(root);
    match fs::metadata(root) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(false),
        Err(error)
            if missing(&error)
                || error.raw_os_error() == Some(rustix::io::Errno::NOTDIR.raw_os_error()) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error),
    }
    let mut stack: Vec<ReadDir> = vec![fs::read_dir(root)?];
    while let Some(entries) = stack.last_mut() {
        let Some(entry) = entries.next() else {
            stack.pop();
            continue;
        };
        let entry = entry?;
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(error) if missing(&error) => continue,
            Err(error) => return Err(error),
        };
        let name = entry.file_name();
        let name = name.as_bytes();
        if kind.is_dir() {
            if !SKIPPED_DIRECTORIES.contains(&name) {
                match fs::read_dir(entry.path()) {
                    Ok(child) => stack.push(child),
                    Err(error) if missing(&error) => continue,
                    Err(error) => return Err(error),
                }
            }
        } else if kind.is_file() && contains(name, PATTERN) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn missing(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_lists_of_usable_paths() {
        assert_eq!(
            parse_paths(br#" ["/a", "-b", "\u00e6"] "#),
            Some(vec!["/a".into(), "-b".into(), "æ".into()])
        );
        assert_eq!(parse_paths(b"[]"), Some(vec![]));
        for raw in [
            &b"{}"[..],
            b"[7]",
            b"[\"\"]",
            b"[\"a\\u0000\"]",
            b"[\"\\ud800\"]",
            b"[\"a\"] [\"b\"]",
            b"\xef\xbb\xbf[]",
            b"[\"\xff\"]",
            b"",
            b"null",
        ] {
            assert_eq!(parse_paths(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn the_pattern_is_a_substring() {
        assert!(contains(b"a.sync-conflict-20260101", PATTERN));
        assert!(contains(b".sync-conflict-", PATTERN));
        assert!(!contains(b"a.sync-conflict", PATTERN));
        assert!(!contains(b"", PATTERN));
    }
}
