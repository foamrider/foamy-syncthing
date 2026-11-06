// foamy-kit 0.5.0 c29ac380ca7ffec3cee9bc9c227bfb3c6bd50f7aa697911feb087b0b5a89b193 — do not edit; change foamy-kit and run sync
//! Shared building blocks for foamy plugin helpers (`foamy-<plugin>-helper`).
//!
//! foamy-kit's `FoamyNative` component vendors this crate into a plugin as
//! `kit/rust/foamy-helper`; the plugin's crate depends on it by path:
//!
//! ```toml
//! [dependencies]
//! foamy-helper = { path = "../kit/rust/foamy-helper" }
//! ```
//!
//! Every module keeps the bounds the Python and Bash helpers had: reads are
//! capped, files are written atomically with owner and type checks, locks
//! never block unless asked to, and child processes run with a deadline and
//! an output cap. Nothing here starts an async runtime or a background thread
//! that outlives a call.
//!
//! - [`io`]: bounded reads (whole streams, files, lines with a maximum length).
//! - [`json`]: JSON-lines requests and replies, and [`json::run`], the
//!   "every failure still prints JSON" entry point.
//! - [`fs`]: atomic file writes, private directories, bounded file reads.
//! - [`lock`]: `flock` locks, including under `$XDG_RUNTIME_DIR/<plugin>`.
//! - [`child`]: bounded child processes (deadline, output cap, kill on
//!   timeout, environment control, death with the parent).
//! - [`process`]: this process: `PR_SET_PDEATHSIG`, `setsid`, rlimits, umask.
//!
//! Modules with heavier dependencies (sandboxing, favicon lookup) are added
//! behind cargo features, so a plugin compiles only what it enables.

pub mod child;
pub mod fs;
pub mod io;
pub mod json;
pub mod lock;
pub mod process;

pub use json::Failure;
