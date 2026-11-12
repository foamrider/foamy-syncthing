# Development

Targets Omarchy 4.5, with runtime feature detection only.

## Layout

| Path | What |
|---|---|
| `Service.qml` | The `service` entry point: one per shell. Owns the native core, the install probe and the conflict scan; registers itself in `ServiceRegistry` |
| `service/` | `CoreProcess.qml` (JSON-lines protocol v2, restart backoff), `PackageController.qml` (install probe/terminal), `Facade.js` (snapshot projection) |
| `Panel.qml` | The bar widget, one per bar (per monitor on 4.5). A `FoamyMember` leader feeds the service its settings and owns the `foamy.syncthing` IPC target |
| `ui/` | `SyncthingView.qml` (lazy popup body), `AddFolderForm.qml`, `UnlinkDialog.qml`, `SyncthingProgress.qml` |
| `models/` | Pure JS: `PanelModel.js` (folder rows, summary, formatting), `OverviewModel.js` (tab rows), `Keyed.js` (in-place ListModel sync) |
| `scripts/` | `syncthing-install.sh` |
| `kit/` | foamy-kit copies. Never edit them here: change foamy-kit and run its `sync.sh` (`tests/kit-check.sh` fails on local edits) |
| `native/` | The Rust helper `foamy-syncthing-helper` (`conflicts`, `stream`, the Syncshell core port, and `pick-folder`, the xdg-desktop-portal folder chooser); its contract is the doc comment in `src/main.rs`; `native.pin` is written by the build tool |
| `bin/<arch>/` | The built helper for x86_64 and aarch64 (never edited by hand) |
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` | The Cargo workspace, its lock file and the kit's pinned toolchain |
| `preview/Preview.qml` | Mock states for foamy-kit's preview harness (not loaded at runtime) |

## Rules

- Follow foamy-kit's DESIGN.md: kit components, `FoamyStyle`/`FoamyPalette`/
  `FoamyMotion` tokens, qualified `Commons.Color`, one keyboard cursor, lazy
  popup body. `foamy-kit/check.sh .` lints this.
- Strings: English keys in `i18n/en.json` with Norwegian in `i18n/no.json`;
  counts through `FoamyI18n.tn` (plural entries), decimals through
  `FoamyI18n.number`. Pass names and paths as `{name}` values, never as keys.
- Never store user paths, API keys, Syncthing configuration or machine
  preferences in the repository. Settings go through `FoamyPrefsQueue`;
  the settings page state through `FoamySettingsState`.
- Keep conflict-scan roots on stdin, never in arguments or diagnostics; the
  scanner launches no children, follows root links only, ignores
  `.stversions`/`.stfolder`, and returns index-aligned booleans.
- Nothing polls for the UI while the popup is closed; the install probe runs
  only while Syncthing is offline; the conflict scan runs on folder-set
  changes and the 5-minute timer.
- Don't unlink a user's real folder to test removal: the tests exercise the
  native pause/forget contract with mocks, `tests/fake-core.py` and the
  fake Syncthing in `tests/fake_syncthing.py`.

## Native helper

`foamy-syncthing-helper` is our Rust port of Syncshell's Go core (commit
`7318f1404e765f274df3d079e35d0ad32b5c1701`, v0.1.9) plus the conflict scan.
Nothing tracks upstream automatically; see `AGENTS.md` for the rules
(protocol v2 stays compatible with `service/CoreProcess.qml`, the API key
never leaves the process, binaries only from the plugin's build tool, `kit/native/`).

Rebuild (developers only; needs rustup with Rust 1.99.1, both musl targets,
`file`, and qemu-user binfmt to test the aarch64 build), from the plugin root:

```sh
export CARGO_TARGET_DIR=~/.cache/foamy-native/foamy-syncthing   # never inside the plugin
kit/native/build-native.sh . --test      # cargo fmt --check, clippy -D warnings, cargo test
kit/native/build-native.sh .             # both arches into bin/<arch>/, writes native/native.pin
kit/native/build-native.sh . --verify    # clean rebuild, byte compare
python3 -I -m unittest discover -s tests -p 'test_*.py'
FOAMY_HELPER='["bin/aarch64/foamy-syncthing-helper"]' python3 -I -m unittest discover -s tests -p 'test_*.py'
bash tests/kit-check.sh
```

Format only this crate (`cargo fmt -p foamy-syncthing-helper`, never
`--all`). After a dependency change run `cargo update -p <crate>` in
`native/` and regenerate `LICENSE-NATIVE-DEPENDENCIES` from the crates that
link into the binaries (`cargo tree -e normal --target <triple>`, without
proc macros).

Porting an upstream Syncshell fix by hand:

1. Read the upstream `core/` change. The Rust modules keep Syncshell's
   names: `internal/session` is `native/src/core/session.rs`, `actions.rs`,
   `activity.rs` and `events.rs`; `internal/syncthing` is `client.rs`,
   `discovery.rs`, `http.rs` and `tls.rs`; `internal/systemduser` is
   `systemd.rs`; `internal/protocol` is `protocol.rs`; Go standard-library
   behaviour the core depends on is in `gocompat.rs`.
2. Port only what the panel uses. A new protocol field or action needs a
   matching change in `service/CoreProcess.qml`, `service/Facade.js` or
   `models/*.js` and stays backwards compatible within protocol v2.
3. Add a case to `tests/test_stream.py` (black-box, fake Syncthing) and,
   for parsing or bounds, a cargo unit test; run the commands above.
4. Against a throwaway Syncthing (`syncthing generate --home <tmp>` and
   `syncthing serve --home <tmp> --no-browser --gui-address=127.0.0.1:<port>`,
   discovery, relays and NAT off, a fake `systemctl` on `PATH`), with TLS on
   and off, compare the snapshots and action results with the previous
   build. Never use your own Syncthing or its configuration.
5. Rebuild, then update README "Native core", the CHANGELOG and, when the
   upstream license text changes, `LICENSE-SYNCSHELL`.

## Tests

See README "Validate". `tests/runtime.py` and the kit's `preview/i18n_runtime.py`
load the real QML offscreen in temporary directories; set `FOAMY_KIT` if foamy-kit
isn't at `~/GitHub/Personal/foamy-kit`. They run on Omarchy 4.5, the installed
shell (`FOAMY_HOST_45` picks another 4.5 tree), and on 4.0 only when
`FOAMY_HOST_40` names a 4.0 shell tree. Screenshots:
`FOAMY_HOST_45=/usr/share/omarchy/shell foamy-kit/preview/render.py . --host 4.5`.
