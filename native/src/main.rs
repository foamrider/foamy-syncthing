//! `foamy-syncthing-helper`: Foamy Syncthing's one native binary, with three
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
//! # `stream` (replaces `bin/x86_64/syncshell-core stream`, protocol v2)
//!
//! A Rust port of the native core of Syncshell
//! (<https://github.com/omarchy-QOL/syncshell>, MIT, © IlyaZar), following
//! `core/` at commit `7318f1404e765f274df3d079e35d0ad32b5c1701` (v0.1.9, see
//! `LICENSE-SYNCSHELL`). Its output is byte for byte the Go core's for the
//! same Syncthing replies (field order, `omitempty`, `null` for empty lists,
//! Go's `<`/`>`/`&` escaping); only the `hello` build version differs.
//!
//! - Flags (Go `flag` syntax: `-x` or `--x`, `=value` or the next argument,
//!   booleans only as `=value`; the last repetition wins): `--config`,
//!   `--endpoint`, `--credential-file`, `--tls-certificate`,
//!   `--expected-device-id`, `--insecure-tls`, `--lifecycle-kind`
//!   (`systemd-user`), `--lifecycle-authorized`, `--lifecycle-unit`,
//!   `--lifecycle-config`, `--probe-interval-seconds` (1–3600, default 15),
//!   `--desired-service-state` (`enabled`|`disabled`), `--json` (accepted,
//!   ignored). A lifecycle binding needs `--lifecycle-authorized`,
//!   `--lifecycle-kind systemd-user` and a unit. Not ported:
//!   `--desktop-authorized` (the desktop bridge) and the `probe`/`status`
//!   commands. A bad invocation, a configuration that can't be found or read,
//!   or an unusable TLS certificate prints `foamy-syncthing-helper: <reason>`
//!   on stderr and exits 1 before `hello`.
//! - Discovery: `--config`, else `--endpoint` with a private (`0600`, not a
//!   link, ≤ 4 KiB) `--credential-file`, else the one existing `config.xml`
//!   among `$STCONFDIR`, `$STHOMEDIR`, `${XDG_STATE_HOME:-~/.local/state}/syncthing`,
//!   `${XDG_CONFIG_HOME:-~/.config}/syncthing` and `~/.local/share/syncthing`;
//!   with several or none, `syncthing paths` (3 s, 64 KiB) decides; then
//!   SyncThingy's Flatpak paths (which never match the native unit). The API
//!   key comes from `config.xml` (≤ 1 MiB) and stays in this process: it is
//!   sent only as `X-API-Key` on authenticated requests to the selected
//!   endpoint (never to `/rest/noauth/health`, never across a redirect to
//!   another origin) and never appears in argv, the environment, stdout,
//!   stderr, snapshots or errors.
//! - Transport: loopback (or any configured) HTTP/1.1 or a Unix socket (a
//!   `unix://` URL's path percent-decoded like Go's `URL.Path`), no proxy;
//!   3 s connect (a host name's lookup included; at most 8 lookups in
//!   flight), 4 s TLS handshake, 8 s to the response headers,
//!   15 s per request (70 s for the event long-poll), all absolute (a peer
//!   that trickles bytes can't stretch them), 8 MiB per response, two idle
//!   keep-alive connections. With TLS on, the GUI certificate
//!   `https-cert.pem` next to `config.xml` is pinned (exact certificate,
//!   within its validity period, handshake signatures verified, no
//!   resumption); `--insecure-tls` only for an explicit HTTPS endpoint;
//!   system roots for an explicit HTTPS endpoint without a certificate.
//! - stdin requests and stdout frames are one JSON object per line, at most
//!   8 MiB with the newline. Requests: `refresh`, `configure` (live
//!   probe/refresh intervals and desired state), `action` and `shutdown`;
//!   frames: `hello`, `snapshot`, `result`, `end`, `fatal`. Ported actions:
//!   `folder.pause`, `folder.resume`, `folder.forget`, `folder.add-existing`,
//!   `folder.suggest-id`, `folder.recheck-errors`, `lifecycle.start`,
//!   `lifecycle.stop`; every other name is `unsupported_action`.
//! - Children: `systemctl --user show <unit> --no-pager --property=LoadState
//!   --property=ActiveState --property=UnitFileState --property=FragmentPath
//!   --property=ExecStart --property=Environment` (3 s) and
//!   `systemctl --user start|stop <unit>` (10 s), only for the authorized
//!   unit; `syncthing paths` during discovery. Each gets stdin `/dev/null`,
//!   its own process group (killed as a group on timeout), death with this
//!   process, and its output captured, capped at 64 KiB and never shown.
//! - Exit: 0 after `end` (`shutdown` or end of stdin); 1 after `fatal` or
//!   when stdout fails. `RLIMIT_CORE` is 0. SIGTERM ends it at once.
//!
//! The module documentation in `core/` gives each part's details.
//!
//! # `pick-folder [title [accept label]]` (replaces `scripts/syncthing-folder-picker.sh`)
//!
//! The desktop's folder chooser through xdg-desktop-portal (`portal.rs`):
//! one JSON line, `{"ok": true, "path"}`, `{"ok": true, "cancelled": true}`
//! or `{"ok": false, "code", "error"}` (`INVALID_ARGUMENT`,
//! `NO_SESSION_BUS`, `PORTAL_UNAVAILABLE`, `PORTAL_FAILED`) with exit 1.
//! Talks only to the session bus (no child processes, no files); waits up to
//! 30 minutes for the choice and dies with its parent.

mod conflicts;
mod core;
mod dbus;
mod portal;

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
        Some("stream") => core::main(&args[1..]),
        Some("pick-folder") => foamy_helper::json::run(
            || portal::main("foamy-syncthing-helper", &args[1..]),
            foamy_helper::json::error_reply,
        ),
        _ => {
            eprintln!("foamy-syncthing-helper: expected conflicts, stream or pick-folder");
            ExitCode::from(2)
        }
    }
}
