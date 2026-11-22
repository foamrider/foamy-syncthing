# Changelog

## Unreleased

- Show the Folders and Devices tabs as the kit's tab row under the summary:
  tabs as wide as their label, the open one with its label in bold and its
  icon, count and underline in the accent, on a hairline across the popup.
  The row is one keyboard stop: ←/→ move between the tabs, Enter opens one
  and Down goes on to the rows. Before, each tab was a stop of its own and
  ←/→ switched the tab at once.
- Offer start and stop for a Syncthing whose home directory has a space, and
  only through its own `syncthing.service`: the core now reads the unit's
  command line and environment from systemd's bus, with every argument
  whole. Such a home got no start/stop button before, and a unit for a
  longer directory (`/srv/Sync -archive` for a selected `/srv/Sync`) got the
  button for another Syncthing.
- Keep the status, the service probe and the activity rotation moving while
  files change: the `LocalIndexUpdated` events of one poll now wait up to
  150 ms in all for deletions to land, not 150 ms per file (up to 2.4 s per
  event), and a file lookup that gets no answer from Syncthing ends the
  lookups of its batch instead of each of up to 48 more waiting for its own
  15 s deadline.
- Show the newest file activity during a large sync: past 12 records the
  oldest one goes, not the one whose key sorts first, which could be the file
  that had just started.
- Refuse a chunked Syncthing reply whose chunk size line is not hex digits
  ending in CRLF (a blank or a sign before the digits, a bare LF), whose
  chunk data does not end in CRLF, whose chunk size lines carry more than
  16 KiB beyond what their data justifies, or whose trailers pass 8 KiB, as a
  connection failure.
- Start the native core once when the popup opens or the bar is
  right-clicked after the core stopped on a protocol error. It was started
  twice, and the first core was stopped right after it came up.
- Show a failed folder chooser's message on the Add folder page, where the
  chooser opens, instead of only on the Settings page behind it. A new
  attempt, or leaving the page, clears the message.
- Clear "Another folder operation is already running" when the running folder
  operation succeeds, so the success notice no longer shows next to it.
- Show the panel-limits warning when the core leaves out folder errors over
  its limit, as it already did for the other cut lists.
- Stop the installation check after 10 seconds, with the programs it
  started, and report it as failed until a check succeeds. A check that hung
  kept every later check from running, and the next check erased a failed
  check's message before the popup showed it.
- Put the caret after the path the folder chooser fills in on the Add folder
  page, so typing extends the path and a long path shows its end. The caret
  was at the start: a typed key went in front of the path.
- Clear a refused folder change's "Syncthing must be online …" message when
  Syncthing is back. It stayed until the next folder operation or a
  right-click.
- Show a failed installation check only while Syncthing is offline. When
  the last check before Syncthing came online had failed, its message stayed
  in the popup, in red, while Syncthing was up to date.
- Close the Add folder page when the add succeeds, also when a recheck that
  started just before it (a right-click on the bar while the page is open)
  failed. The page stayed open with the recheck's message, as if the add had
  failed.
- End a hung installation check, with the programs it started, when the
  plugin's service goes during it (a shell reload, the plugin turned off).
  Only its `timeout` was stopped, so the check ran on without a deadline.
- Sync foamy-kit 0.13.1: a click beside an Add folder field ends its edit,
  long tooltips wrap and are cut with "…", and a row opened at the end of
  the list scrolls fully into view.

## Foamy Syncthing 1.4.0

- Show this computer's Syncthing version at the end of the popup header
  ("1 / 2 devices online · v2.0.10") and a connected device's version on its
  row ("3 shared folders · v2.0.9"). The core reads the version once per
  connection and per Syncthing restart and keeps it only when it is at most
  40 printable ASCII characters; a failed read leaves it out instead of
  failing the whole refresh.
- Count only the other devices in the header, as the Devices tab does. It
  counted this computer too.
- Leave out header lines that the tabs and rows already show: the folder
  count under "Up to date", the count of paused folders, "Waiting for the
  local service", and a header line that repeated the title while Syncthing
  was unreachable.
- "Installation needs cleanup" names the file in the way: a leftover
  `~/.local/bin/syncthing` or user `syncthing.service`.
- Mark conflicts in folders that Syncthing keeps as `~/…`: the conflict scan
  now expands the path first, as the folder rows do.
- Allow one install at a time with a lock in `$XDG_RUNTIME_DIR`, which ends
  with its process, instead of a pid file that another run could delete.
  Without `XDG_RUNTIME_DIR` the install script fails with a message instead
  of using `/tmp`.
- Remove the `serviceState` setting ("Intended Syncthing autostart state"),
  which did nothing. A saved value is ignored.
- Read the core's `--credential-file`, which the panel does not use,
  through one descriptor: a file swapped in after the check is never read,
  and a file over 4 KiB is refused without reading all of it.
- Sync foamy-kit 0.9.0. `THIRD_PARTY_NOTICES.md` also lists the crates of
  the Rust standard library that the helper links.

## Foamy Syncthing 1.3.2

- A setting saved through `omarchy bar set`, when the in-place save is not
  available, always ends: a save that can't start or runs past 20 seconds is
  stopped with the programs it started and reported, and later saves no
  longer wait for it forever.
- Ship `THIRD_PARTY_NOTICES.md`: every crate linked into the helper, per
  architecture, with its license and notice texts, and the Rust standard
  library's, musl's and LLVM's.
- Sync foamy-kit 0.8.0.

## Foamy Syncthing 1.3.1

- GitHub rebuilds both helper binaries for every push, compares them byte
  for byte with `bin/` and attests them, so `gh attestation verify` checks
  the committed files. The README says how to verify them yourself, and how
  to build and run your own.
- Rebuild the helper on foamy-helper 0.1.1: a child process that it killed
  after an error is now reaped.
- Sync foamy-kit 0.7.0.

## Foamy Syncthing 1.3.0

- **Choose an existing directory** asks the desktop's folder chooser through
  xdg-desktop-portal, from the helper's new `pick-folder` subcommand, instead
  of a `qml6` window that a background `hyprctl`/`jq` loop floated and
  centred. `qml6` and `hyprctl` are no longer needed; the removed
  `scripts/syncthing-folder-picker.sh` and `scripts/folder-picker.qml` go
  with them. A missing session bus or portal now says so instead of the
  generic failure.

## Foamy Syncthing 1.2.0

- Replace the bundled Go core (Syncshell 0.1.9, `bin/x86_64/syncshell-core`)
  and the Python conflict scanner with one static Rust helper,
  `bin/<arch>/foamy-syncthing-helper`, for x86_64 and **aarch64**. Its
  `stream` subcommand is a Rust port of Syncshell's core at commit 7318f140
  (MIT, credited in `LICENSE-SYNCSHELL`): the same protocol v2 and, for the
  same Syncthing replies, the same output; the API key still never leaves
  the process, and the GUI certificate is still pinned when TLS is on. Only
  what the panel uses is ported; the unused desktop bridge, `probe`/`status`
  and Syncshell's other actions are gone. 2.3 to 2.8 MB instead of 11.8 MB,
  and the Go build's ten known standard-library vulnerabilities go with it.
  Python 3 is no longer needed at runtime. Other CPUs show "This plugin's
  helper isn't available for this CPU." instead of an architecture error.
- Bound every request in the native core by absolute deadlines, as the Go
  core's transport did: 4 s for the TLS handshake, 8 s to the headers and the
  request's own limit for the body, so an endpoint that trickles bytes can no
  longer hold a refresh. Host name lookups and Unix socket connects count
  against the 3 s dial bound, and `unix://` URL paths are percent-decoded.
- Keep the API key out of the core's debug output.
- Name every CPU instruction HTTPS needs on an unsupported CPU (AES and
  carry-less multiply, AVX2, BMI2 and ADX on x86-64, or the ARMv8 crypto
  extensions), from foamy-kit 0.5.2.
- Target Omarchy 4.5 only in the documentation; the offscreen tests run on the
  installed 4.5 shell.
- Sync foamy-kit 0.5.2.

## Foamy Syncthing 1.1.0 - Unreleased

- Use foamy-kit for the popup. The forked 4.0 KeyboardPanel landed at the
  wrong edge on Omarchy 4.5, and its bare `Color.` references showed a white
  card with invisible text under Qt 6.12. The content is now built on first
  open, with one keyboard cursor, Tab to switch bar panels, and theme status
  colours instead of Nord hexes.
- Show Syncthing's folder labels; mark folders that contain sync-conflict
  files.
- Scan for conflicts only when the folder set changes (and every 5 minutes),
  and ignore `.stversions`/`.stfolder`. Check the Syncthing installation only
  while Syncthing is offline.
- Keep restarting a crashed native core with backoff instead of giving up
  after three tries; opening the popup or right-click restarts it at once.
- Start the native core without the unused desktop bridge
  (`--desktop-authorized`).
- Stop sending desktop notices for actions taken in the popup.
- Add `barWidget.schema` for the interval and warning settings, and refresh
  intervals on the Settings page; save settings in place.
- Translate service and core messages, with plurals and localized decimals.
- Pin the native core's provenance (Syncshell v0.1.9, commit 7318f140,
  go1.27.1) and add `packaging/bundled/build-core.sh` and `verify-core.sh`.
- Remove the unused Syncshell panel, Web UI theme files and assets (about
  7,000 lines of QML/JS); flatten the adapter into `Service.qml` and
  `service/`.

## Foamy Syncthing 1.0.0

- Add the themed overview, folder settings, and existing-directory form.
- Separate pause/resume from confirmed unlink, preserving local files.
- Support English and Norwegian Bokmål, following Omarchy 4.5's language.
- Store plugin preferences through Omarchy shell.json under `foamy.syncthing`.
- Bundle the unmodified Syncshell 0.1.9 native core and retain upstream licenses.

## Upstream Syncshell history

The following entries describe the bundled upstream project.

## 0.1.9 - 2026-09-21

- add remote-device discovery and setup to the Omarchy panel, including nearby
  and pending devices
- add folder-sharing controls for new and existing folders and safe local
  removal of configured devices and folder shares
- configure incoming folder offers and confirm creation of missing local
  directories, including paths starting with `~`
- confirm folder link changes and device removal before applying them
- allow clean plugin removal when Syncthing is stopped, disabled, or absent
- report only remote devices in connection totals and use Syncthing's Device ID
  terminology
- refine panel dropdowns, status indicators, action placement, and installation
  feedback
- keep folder controls inert during targeted and all-folder rescans and handle
  paused folders and long-running scans consistently

## 0.1.8 - 2026-09-13

- detect SyncThingy Flatpak installations, adapted to the Go core from
  [@whelanh](https://github.com/whelanh)'s contribution in
  [#48](https://github.com/omarchy-QOL/syncshell/pull/48); preserve native
  discovery precedence, validate config/TLS, and guard native service controls
- fix themed bar icon contrast and rendering at the host size, contributed by
  [@baranskyi](https://github.com/baranskyi) in
  [#47](https://github.com/omarchy-QOL/syncshell/pull/47)
- stop zero-work `sync-preparing` transitions from flashing as active syncing,
  contributed by [@dawez](https://github.com/dawez) in
  [#101](https://github.com/omarchy-QOL/syncshell/pull/101)

- connect local conflict folder opening and guarded renaming to the desktop
  user, without changing permissions or replacing an existing destination
- label conflict resolution as beta and use a themed Syncshell header

- add a Preact Web UI with grouped folders/devices, notifications and
  API-based conflict discovery and rescans
- refine the 400-pixel panel action area, keep current and planned interface
  entries visible, standardize tooltips and acknowledge status refreshes
- migrate older plugin settings with a preview and an exact backup while
  preserving valid preferences, comments, permissions and managed symlinks
- show folder error details and theme the selected Web UI address
- stop the rescan wait with an error when Syncthing becomes unavailable
- report rejected service actions immediately and keep their errors visible
- enforce the core crash-retry limit without blocking initial installation
  recovery, and disable unavailable actions after the core stops
- check existing tilde folder paths for overlap using Syncthing's own home
  directory; require absolute paths when their location cannot be determined
- resume file activity when Syncthing's event sequence resets on reconnect
  while preserving unread events from a surviving daemon
- match Syncthing home and config directory options consistently so service
  controls cannot target another instance through short option spellings
- replace the QML Syncthing engine with one parent-bound native Go core
- fix issue 45 by treating healthy external Syncthing instances as online
  without exposing controls for an unrelated inactive user service
- bundle one reproducible static Linux x86_64 executable with a public JSONL
  protocol and keep the panel's credentials, REST, events and lifecycle in Go
- make accepted single and global rescans visible immediately with rotating,
  inert controls and explicit optimistic folder state
- keep long-running accepted rescans from surfacing a false request timeout
- wait until targeted folders leave scanning before reporting rescan
  completion, so notices and the all-folders control follow the last folder
  rather than the HTTP accept
- show the bar sync overlay as soon as a plugin rescan is busy, not only
  after snapshot scanning arrives
- distinguish plain `SCANNING` from concurrent `SCAN+SYNC` during an accepted
  rescan
- require a shell restart after updating to load the current plugin while
  preserving settings and bar placement

## 0.1.7 - 2026-08-31

- reconcile configured and systemd user-service startup states without changing
  the independent runtime start and stop controls (thanks @renews for reporting)
- add a configurable interval for detecting external systemd state changes
- add per-folder and all-folder rescans, a clearer host ID copy control, and
  remembered local host identity while Syncthing is stopped

## 0.1.6 - 2026-08-22

- Make live and indexed file activity accurate for additions, deletions,
  concurrent changes, and active folders.
- Refine folder management with pending offers, copyable Syncthing IDs, and
  explicit link, unlink, and forget actions.
- Add versioned icon preferences, live Omarchy Web UI theming, and clean plugin
  removal that can preserve settings.
- Refresh the preview and add four focused demo videos.

## 0.1.5 - 2026-08-20

- Add an optional theme-colored bar icon while keeping the Syncthing artwork as
  the default (@davidszp, @ilyaZar).

## 0.1.4 - 2026-08-16

- Support Syncthing installations with TLS enabled for the local API.
- Clarify installation state and simplify plugin instructions.

## 0.1.3 - 2026-08-15

- Add a demo video and improve the documentation.

## 0.1.2 - 2026-08-15

- Add folder management to the bar panel.

## 0.1.1 - 2026-08-14

- Keep Syncthing package management explicit and monitor existing installs.
- Show live file activity and refresh status after synchronization.

## 0.1.0 - 2026-08-12

- Initial release.
