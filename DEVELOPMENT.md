# Development

Targets Omarchy 4.5 and keeps loading on 4.0.6 (English there), with runtime
feature detection only.

## Layout

| Path | What |
|---|---|
| `Service.qml` | The `service` entry point: one per shell. Owns the native core, the install probe and the conflict scan; registers itself in `ServiceRegistry` |
| `service/` | `CoreProcess.qml` (JSON-lines protocol v2, restart backoff), `PackageController.qml` (install probe/terminal), `Facade.js` (snapshot projection) |
| `Panel.qml` | The bar widget, one per bar (per monitor on 4.5). A `FoamyMember` leader feeds the service its settings and owns the `foamy.syncthing` IPC target |
| `ui/` | `SyncthingView.qml` (lazy popup body), `AddFolderForm.qml`, `UnlinkDialog.qml`, `SyncthingProgress.qml` |
| `models/` | Pure JS: `PanelModel.js` (folder rows, summary, formatting), `OverviewModel.js` (tab rows), `Keyed.js` (in-place ListModel sync) |
| `scripts/` | `syncthing-conflicts.py`, `syncthing-install.sh`, the folder chooser |
| `kit/` | foamy-kit copies. Never edit them here: change foamy-kit and run its `sync.sh` (`tests/kit-check.sh` fails on local edits) |
| `bin/`, `packaging/bundled/` | The native core, its checksum, provenance pin, build and verify scripts |
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
  native pause/forget contract with mocks and `tests/fake-core.py`.

## Updating the native core

1. Pick an upstream Syncshell tag; read its `core/` diff (protocol v2 must
   stay compatible with `service/CoreProcess.qml`).
2. Update `packaging/bundled/core.pin` (tag, commit, Go release, x/sys pin).
3. `packaging/bundled/build-core.sh --install` with the pinned Go release,
   then `packaging/bundled/verify-core.sh`. Compare the result with the
   upstream release binary.
4. Update README "Native core", CHANGELOG and `LICENSE-SYNCSHELL`.

## Tests

See README "Validate". `tests/runtime.py` and `tests/i18n-runtime.py` load
the real QML offscreen in temporary directories; set `FOAMY_KIT` if foamy-kit
isn't at `~/GitHub/Personal/foamy-kit`. Screenshots:
`foamy-kit/preview/render.py . --host both`.
