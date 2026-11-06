//! `foamy-syncthing-helper`: Foamy Syncthing's one native binary, with two
//! subcommands. Anything else (no subcommand, another one) prints a usage
//! line to stderr and exits 2. A helper bug exits 101 from the panic hook
//! (message on stderr, no core dump). It never exits 126 or 127, which the
//! panel reserves for "no helper for this CPU" (kit `FoamyNative`).
//!
//! # `conflicts` (replaces `scripts/syncthing-conflicts.py`, same contract)
//!
//! - argv: exactly `conflicts`; any further argument prints
//!   `Conflict helper accepts stdin only.` and exits 2.
//! - stdin: a JSON list of folder roots, at most 1 MiB, each a non-empty
//!   string without NUL (strict UTF-8, no BOM, no trailing data). Paths never
//!   go in argv.
//! - stdout: one line, the index-aligned flags in Python's `json.dumps`
//!   spelling, e.g. `[false, true]`; exit 0. Any failure (input, a root or
//!   directory that can't be read, the deadline) prints nothing on stdout,
//!   `Could not check for Syncthing conflicts.` on stderr (never a path) and
//!   exits 1.
//! - A root is clear when it is missing or not a directory. The walk follows
//!   the root if it is a link but no link inside it, skips directories named
//!   `.stversions` or `.stfolder`, counts only regular files whose name
//!   contains `.sync-conflict-`, streams each directory depth first and stops
//!   at the first match; an entry that vanishes mid-walk is skipped.
//! - Deadline: 25 s (the panel kills the scan at 30 s anyway). Test-only:
//!   `FOAMY_SYNCTHING_CONFLICTS_DEADLINE=<1..25>` shortens it.
//! - Runs no child processes, reads no other files, writes nothing.
//!
//! # `stream`
//!
//! The native core (a Rust port of Syncshell's core) follows in the next
//! commits; until the panel switches to it, `stream` only reports that it
//! is not available (exit 1).

mod conflicts;

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    // panic = "abort" would end a helper bug in SIGABRT and a core dump (of
    // a process that holds the API key): exit 101 from the hook instead.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        std::process::exit(101);
    }));
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|command| command.to_str()) {
        Some("conflicts") => conflicts::main(&args[1..]),
        Some("stream") => {
            eprintln!("foamy-syncthing-helper: stream is not available yet");
            ExitCode::from(1)
        }
        _ => {
            eprintln!("foamy-syncthing-helper: expected conflicts or stream");
            ExitCode::from(2)
        }
    }
}
