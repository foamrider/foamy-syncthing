# Foamy Syncthing: notes for coding agents

An Omarchy shell plugin (QML) with one bundled native helper,
`foamy-syncthing-helper` (Rust, `native/`). README.md describes the plugin,
DEVELOPMENT.md the layout and the build; this file is the short list of
rules a change must keep.

## The native core is our Rust port of Syncshell

- `foamy-syncthing-helper stream` is a port of the Go core of Syncshell
  (https://github.com/omarchy-QOL/syncshell, MIT, by IlyaZar) at commit
  `7318f1404e765f274df3d079e35d0ad32b5c1701` (v0.1.9). Keep the credit in
  `LICENSE-SYNCSHELL`, README "Native core" and the crate documentation.
- Nothing tracks upstream. A Syncshell fix is carried over by hand
  (DEVELOPMENT.md, "Porting an upstream Syncshell fix by hand"), and only
  for what the panel uses.
- `foamy-syncthing-helper conflicts` is the conflict scan: roots on stdin
  only, never in argv or diagnostics; it starts no programs.
- `foamy-syncthing-helper pick-folder` is the folder chooser: one
  xdg-desktop-portal `OpenFile` request (`directory: true`) on the session
  bus, the same `native/src/portal.rs` and `dbus.rs` as foamy-github (from
  foamy-lock's studio). Never run it against the real session bus in tests:
  it opens a dialog on the user's screen. The unit tests use a fake bus.

## Rules for changing the core

- **Protocol v2 stays compatible** with `service/CoreProcess.qml`,
  `service/Facade.js` and `models/*.js`: the frame types (`hello`,
  `snapshot`, `result`, `end`, `fatal`), field names, `null` for empty
  lists, strictly increasing revisions, the 8 MiB line bound, strict
  requests. Add fields only in a backwards-compatible way, together with the
  QML/JS that reads them.
- **The API key never leaves the process**: not in argv, the environment,
  stdout, stderr, snapshots, errors, logs or `Debug` output; it goes only
  into the `X-API-Key` header of authenticated requests to the selected
  endpoint (never on `/rest/noauth/health`, never across a cross-origin
  redirect). With TLS on, the GUI certificate stays pinned; `--insecure-tls`
  stays limited to an explicit HTTPS endpoint.
- **Lifecycle**: only `systemctl --user show|start|stop <unit>` for the unit
  the plugin authorizes on the command line (`--lifecycle-authorized`).
  Test it with a fake `systemctl` on `PATH` (tests/core_harness.py); never
  against the real user service.
- Keep every bound (timeouts, output caps, list limits, string lengths) on
  purpose; no async runtime; same system tools with the same argv.
- **Binaries come only from the plugin's build tool** (`kit/native/`), never by hand:
  `kit/native/build-native.sh . --test`, `kit/native/build-native.sh .`,
  `kit/native/build-native.sh . --verify` (byte-identical rebuild). Commit
  `bin/x86_64/…`, `bin/aarch64/…` and `native/native.pin` with the source
  change. Never write a Cargo `target/` inside the plugin
  (`CARGO_TARGET_DIR=~/.cache/foamy-native/foamy-syncthing`).
- Never touch a real Syncthing, its `config.xml` or its API key in tests
  or by hand: use `tests/fake_syncthing.py`, or a throwaway
  `syncthing serve --home <tmp> --no-browser --gui-address=127.0.0.1:<port>`.
- `kit/` holds foamy-kit copies: change foamy-kit and run its `sync.sh`.

## Tests

```sh
export FOAMY_KIT=~/GitHub/Personal/foamy-kit   # or the foamy-kit checkout in use
node --test tests/*.test.js tests/*.test.cjs
python3 -I -m unittest discover -s tests -p 'test_*.py'
FOAMY_HELPER='["bin/aarch64/foamy-syncthing-helper"]' python3 -I -m unittest discover -s tests -p 'test_*.py'
python3 tests/runtime.py
python3 tests/i18n-runtime.py
bash tests/kit-check.sh
$FOAMY_KIT/check.sh .
omarchy plugin validate .
```

`tests/test_stream.py` is the core's black-box contract (fake Syncthing,
fake `systemctl`); `tests/test_conflicts.py` the scan's.

<!-- foamy-kit review: begin; managed by foamy-kit sync.sh, edit outside this block -->
## Code review

Follow [REVIEW.md](REVIEW.md) when reviewing or preparing a change. It is the
shared Foamy review standard, synced from foamy-kit. The rest of this file
adds repository-specific rules.
<!-- foamy-kit review: end -->
