# Foamy Syncthing

A Syncthing plugin for Omarchy Quattro, using the `foamy.syncthing` namespace.
A compact themed overview shows folders, synchronization progress, and connected
devices. Settings provides folder actions, a directory form, and language selection.

## Installation

```sh
omarchy plugin add https://github.com/foamrider/foamy-syncthing.git --enable
```

## Local development

Requires Omarchy Quattro on Linux x86_64, Syncthing, Bash, jq, coreutils, and Qt 6
with Qt Quick Dialogs (`qml6`). The native Syncshell core is bundled; no Go build
or runtime download is needed. Syncthing's user service and configuration remain
owned by Syncthing. If Syncthing is absent, the footer action uses Omarchy's
normal package installation flow.

From this checkout (the destination must not already exist):

```sh
omarchy plugin validate .
mkdir -p ~/.config/omarchy/plugins
ln -s "$PWD" ~/.config/omarchy/plugins/foamy.syncthing
omarchy restart shell
omarchy plugin enable foamy.syncthing
```

Disable another Syncthing shell plugin before enabling this one. Development
edits require `omarchy restart shell` before live verification. The installed plugin reads this checkout directly; no copy or reinstall is needed.

## Controls

- Left-click opens the overview; right-click refreshes status.
- The cog opens settings. The globe opens the existing Syncthing Web UI.
- Pause/Resume keeps a folder's configuration, sharing, and files.
- Unlink asks for confirmation, pauses if needed, and removes the folder from
  Syncthing while keeping local files. Syncthing may remove its internal
  `.stfolder` marker. If removal fails after pausing, the folder stays paused and
  the error states that explicitly. Reuse the original folder ID to rejoin it.
- Add folder selects an existing directory, identity, and devices. It does not
  create directories. Incoming offers retain their folder ID and originating device.
- The footer's play/pause button starts/stops the local Syncthing service.
  When stopped, a red dot and service message replace the connected-device count.
- The popup grows to fit the screen, up to 900 scaled pixels. Only folder lists
  scroll; headings, preferences, and footer stay fixed.

Native events update folder state; service health refreshes every five seconds
while open. Byte progress is shown only when measurable. Scanning, unavailable
states, and metadata-only work do not claim completion. A completed folder set
shows a green checkmark. The core does not expose transfer-rate counters.

R refreshes, W opens the Web UI, P starts/stops the service, M opens settings,
and Q closes the panel. Tab navigates settings controls; Escape returns to the
previous view. IPC uses `omarchy-shell foamy.syncthing toggle` and
`omarchy-shell foamy.syncthing settings`. Diagnostics:
`omarchy-shell foamy.syncthing.status state`.

## Preferences and languages

The settings view offers English, Norwegian Bokmål, or the system language.
Norwegian locales (`nb`, `nn`, `no`) use Bokmål; other unsupported locales use
English. User-provided names, paths, IDs, and technical errors returned by
Syncthing/native dependencies are preserved as received. Plugin UI, controls,
confirmations, and locally generated notices are translated.

All plugin preferences live on this widget's entry in
`$XDG_CONFIG_HOME/omarchy/shell.json` (`~/.config` by default). Changes use
`omarchy bar set <installed-id> <key> <value> --json`; the plugin never rewrites
shell.json directly. The manifest supplies the installed ID. No plugin TOML
file is read or created, and no Web UI theme is changed by this plugin.

| Key | Default | Supported values |
| --- | --- | --- |
| `language` | `system` | `system`, `en`, `nb` |
| `minimumConnectedDevices` | `0` | 0 disables warnings; 1–100 includes this computer |
| `refreshIntervalSec` | `60` | 60–3600 seconds for background refresh |
| `openRefreshIntervalSec` | `5` | 2–60 seconds while the panel is open |
| `probeIntervalSec` | `15` | 5–300 seconds for service-state probes |
| `serviceState` | `enabled` | `enabled`, `disabled`; desired user-service state |

Only language and the device warning threshold need controls in the settings
view. Advanced values can be changed with `omarchy bar set`. Syncthing credentials,
folder/device configuration, and runtime state remain outside the plugin and
are handled by Syncthing and the bundled core. No personal directories, peers,
credentials, or warning threshold are included in this distribution.

## Development and attribution

```sh
node --test tests/*.test.js
bash tests/test-preferences.sh
omarchy plugin validate .
sha256sum -c packaging/bundled/SHA256SUMS
```

The native backend and adapter originate from
[Syncshell](https://github.com/omarchy-QOL/syncshell) 0.1.9, revision
`7318f1404e765f274df3d079e35d0ad32b5c1701`. The Linux x86_64 binary is unmodified;
its checksum is in `packaging/bundled/SHA256SUMS` (run the checksum command from
this plugin's root, using `sha256sum -c packaging/bundled/SHA256SUMS`).
The adapter's settings boundary has been changed to use Omarchy shell.json.
The copied Omarchy popup retains its focus, geometry, and dismissal lifecycle.

Code is MIT-licensed; retain `LICENSE`, `LICENSE-SYNCSHELL`, and
`LICENSE-OMARCHY`. The bundled Go runtime has its BSD license in
`packaging/bundled/LICENSE.golang`. The inherited Web UI assets retain their
MIT, MPL-2.0, OFL-1.1, and BSD notices inside `webui/`. See `DEVELOPMENT.md`
for source boundaries and validation.
