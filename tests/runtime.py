#!/usr/bin/env python3
"""Load the real Service.qml once and Panel.qml three times (one bar per
monitor) in an isolated offscreen Quickshell against a host shell, through
foamy-kit's harness, with tests/fake-core.py in place of the native core and
a counting stub for the install probe.

  python3 tests/runtime.py

The host is Omarchy 4.5, the installed /usr/share/omarchy/shell
(FOAMY_HOST_45 picks another 4.5 tree).

Checks: one service and one settings leader for three bars, and a new leader
when the first bar goes, whose settings reach the core as `configure`; the
core's exact argv (lifecycle authority for syncthing.service, no desktop
bridge); the install probe stops for a full interval once Syncthing is
online; snapshots with the same folders start no conflict walk and a moved
folder starts one; .stversions is ignored and conflicts are flagged per
folder, a "~/" folder path included, with an urgent bar and its tooltip; the
status IPC reply; the popup body is built on open only; the Folders /
Devices tab row is one cursor stop (←/→ between the tabs, Enter opens one,
Down goes on to the rows, a hovered tab takes the cursor); in a dark theme
with a dim red, the hero title reads on the band and an expanded conflict
row's state and detail read under the cursor in one red; row delegates
survive a snapshot (keyed model); a plain p stops syncing and Ctrl+P does
not; settings open; a folder chooser left open goes with its Panel (the real
pick-folder helper, blocked on a session bus that never answers, is gone
once the Panel is destroyed and nothing of it outlives the run), and one the
bus refuses reopens the popup with its error on the Add folder page (it
reads at 4.5:1 in a dark theme with a dim red), which a new attempt or
leaving the page clears; a crash-looping core is
started four times, then retried with a backoff capped at 5 minutes, and
right-click/open starts it now; a leftover user unit blocking the install is
named in the popup summary, not in the bar tooltip, and Install starts
nothing. The live scene: the real core's snapshot reaches the header, the
devices and the service controls. The service scene: a failed installation
check from before Syncthing was online is not shown, pause, resume and unlink
send the right requests in order and settle on failure, a failed recheck
keeps its message through an unlink that succeeds, an add that waits for a
failed recheck still closes its page when it succeeds, a failed stop settles,
the bar colour follows folder problems, too few devices and a Syncthing
outage, folder changes are refused while Syncthing is down and the
refusal's message goes when it is back, every kind of broken core output
stops the core once without a restart until recover(), which starts one
core, and a core that dies ends a held folder action.
Without a helper for this CPU the core and the folder chooser say so and
recover() launches nothing. A path the folder chooser answers lands in the
Add folder field with the caret after it: typing extends it, and a path
wider than the field shows its end. Closed on Settings and opened again, the
popup closes on a real Esc, as it does on the Esc after one that left
Settings. In Settings of a widget without a bar entry, a pick the settings
queue refuses shows the saved value again at once, in each settings choice.
A hung installation check is stopped at its deadline with its child
process, reported as a failure until a check succeeds, and the next check
runs; one still hung when the service goes is stopped with it.
Requires ~/GitHub/Personal/foamy-kit (or FOAMY_KIT)."""

import json
import os
import platform
import shutil
import socket
import struct
import sys
import tempfile
import threading
from pathlib import Path

KIT = Path(os.environ.get("FOAMY_KIT", Path.home() / "GitHub/Personal/foamy-kit"))
# Set before the harness reads them.
os.environ.setdefault("FOAMY_HOST_45", "/usr/share/omarchy/shell")
sys.path.insert(0, str(KIT / "preview"))
import harness  # noqa: E402

import core_harness  # noqa: E402
import fake_syncthing  # noqa: E402

SOURCE = Path(__file__).resolve().parents[1]

PROBE_STUB = """#!/bin/bash
printf '%s\\n' "$1" >> "$FOAMY_PROBE_LOG"
# The first status check hangs in a child process, as a status stuck on a
# filesystem would; the child's PID goes to the file.
if [ -n "${FAKE_PROBE_HANG:-}" ] && [ ! -e "$FAKE_PROBE_HANG" ]; then
  sleep 1000 &
  printf '%s\\n' "$!" > "$FAKE_PROBE_HANG"
  wait
fi
# The first status check fails, as a script error would; the file notes it.
if [ -n "${FAKE_PROBE_FAIL:-}" ] && [ ! -e "$FAKE_PROBE_FAIL" ]; then
  : > "$FAKE_PROBE_FAIL"
  echo "status check failed" >&2
  exit 1
fi
if [ -n "${FAKE_PROBE_BLOCKING:-}" ]; then
  jq -cn --arg blocking "$FAKE_PROBE_BLOCKING" \\
    '{state:"incomplete",label:"Incomplete installation: non-working",executable:"",blocking:$blocking,operationRunning:false}'
else
  printf '{"state":"existing","label":"Existing installation found: working","executable":"/usr/bin/syncthing","blocking":"","operationRunning":false}\\n'
fi
"""

SHELL = r"""
import QtQuick
import Quickshell
import Quickshell.Io
import "harness"
import "plugin/kit" as Kit
import "plugin" as Plugin

ShellRoot {
  id: shell
  property var panels: []
  property var service: null
  property int checks: 0
  property int scans: 0
  property var savedRow: null
  function check(ok, what) { checks++; if (!ok) console.log("FOAMY FAIL " + what) }
  function probes() { return probeLog.text().split("\n").filter(function(l) { return l === "status" }).length }

  FloatingWindow {
    implicitWidth: 1920; implicitHeight: 1080
    Item { id: host; width: 1920; height: 1080 }
  }
  Component { id: barComponent; MockBar { barSize: 34 } }
  FileView { id: probeLog; path: PROBE_LOG; watchChanges: true; printErrors: false; onFileChanged: reload() }
  FileView { id: coreLog; path: CORE_LOG; watchChanges: true; printErrors: false; onFileChanged: reload() }
  FileView { id: pickerLog; path: PICKER_LOG; watchChanges: true; printErrors: false; onFileChanged: reload() }
  FileView { id: snapshotFile; path: SNAPSHOT; blockLoading: true; blockWrites: true; printErrors: false }
  Connections {
    target: shell.service ? shell.service.conflictScan : null
    function onRunningChanged() { if (shell.service.conflictScan.running) shell.scans++ }
  }

  Component.onCompleted: {
    var serviceComponent = Qt.createComponent(Qt.resolvedUrl("plugin/Service.qml"))
    if (serviceComponent.status !== Component.Ready) { console.log("FOAMY FAIL load service " + serviceComponent.errorString()); Qt.quit(); return }
    service = serviceComponent.createObject(null)
    var component = Qt.createComponent(Qt.resolvedUrl("plugin/Panel.qml"))
    if (component.status !== Component.Ready) { console.log("FOAMY FAIL load " + component.errorString()); Qt.quit(); return }
    for (var i = 0; i < 3; i++) {
      var bar = barComponent.createObject(shell, { moduleName: "foamy.syncthing" })
      panels.push(component.createObject(host, { bar: bar, moduleName: "foamy.syncthing", x: 1500, y: 0, width: 40, height: 34,
        settings: { minimumConnectedDevices: 2, probeIntervalSec: 5, openRefreshIntervalSec: 60 } }))
    }
    wait.waitFor(function() {
      return service.online && service.settingsReady && Kit.FoamyShared.memberCount("syncthing") === 3 && shell.probes() >= 1
        && service.folderRows.length === 2 && service.conflictFolderIds.docs === true && !service.conflictScan.running
    }, 20000, step0, "the service comes online with its conflict scan")
  }

  FoamyWait { id: wait }
  property int probesOnline: 0
  property double onlineAt: 0
  // Waits for `condition`, counts it as a check, then goes on (also after a timeout).
  function expect(condition, ms, what, next) { wait.waitFor(condition, ms, function(ok) { check(ok, what); next() }) }
  function requests() {
    return coreLog.text().split("\n").filter(function(l) { return l.indexOf('{"request"') === 0 }).map(function(l) { return JSON.parse(l).request })
  }
  function lifecycleActions() {
    return requests().filter(function(r) { return r.type === "action" && r.action.indexOf("lifecycle.") === 0 }).map(function(r) { return r.action })
  }
  function configured(seconds) {
    return requests().some(function(r) { return r.type === "configure" && r.config && r.config.refreshIntervalSeconds === seconds })
  }
  function step0() {
    check(Plugin.ServiceRegistry.instance === service, "service registered")
    check(Kit.FoamyShared.memberCount("syncthing") === 3, "three members")
    check(service.preferences.minimumConnectedDevices === 2 && service.settingsReady, "leader fed settings")
    check(service.online, "online through the fake core")
    // The argv the real core must accept: lifecycle authority for the one
    // unit, no desktop bridge. The core can start before the leader's
    // settings arrive, so the interval is the default or the saved one.
    var argv = JSON.parse(coreLog.text().split("\n")[0]).argv
    check(argv.length === 8 && argv[0] === "stream" && argv[1] === "--probe-interval-seconds" && /^\d+$/.test(argv[2])
          && Number(argv[2]) >= 5 && Number(argv[2]) <= 300
          && JSON.stringify(argv.slice(3)) === JSON.stringify(["--lifecycle-kind", "systemd-user", "--lifecycle-authorized", "--lifecycle-unit", "syncthing.service"]),
          "the core's argv: " + JSON.stringify(argv))
    check(shell.probes() >= 1, "install probe ran while offline")
    check(JSON.stringify(service.conflictFolderIds) === JSON.stringify({ docs: true }), "per-folder conflicts, .stversions ignored: " + JSON.stringify(service.conflictFolderIds))
    check(service.folderRows.length === 2 && service.folderRows[0].label === "Documents", "configured label first")
    // Escaped icon-font glyphs (\u{…}) still decode: conflicts → the "notify" glyph.
    check(panels[0].barIcon.codePointAt(0) === 0xf09e0 && panels[0].barIcon.length === 2, "bar glyph decodes: " + panels[0].barIcon.codePointAt(0))
    check(Qt.colorEqual(panels[0].iconColor, panels[0].bar.urgent), "conflicts make the bar urgent")
    check(panels[0].tooltip === "Syncthing: Needs attention\nLeft: details · Right: refresh", "bar tooltip: " + JSON.stringify(panels[0].tooltip))
    check(service.tooFewDevices === false && service.deviceCounts.connected === 2, "device counts include this computer")
    // The documented foamy.syncthing.status reply (README).
    var status = JSON.parse(service.diagnostics.state())
    check(status.online === true && status.folders === 2 && status.peers === 1 && status.conflicts === true && status.error === "",
          "foamy.syncthing.status state: " + JSON.stringify(status))
    shell.probesOnline = shell.probes()
    shell.onlineAt = Date.now()
    shell.scans = 0
    // Snapshots with the same folders (refreshes) must not walk again.
    service.refresh(); service.refresh()
    check(panels[0].popup.bodyItem === null && panels[1].popup.bodyItem === null, "bodies lazy before open")
    panels[1].open()
    wait.waitFor(function() { return panels[1].popup.bodyItem !== null && panels[1].popup.bodyItem.rowCount === 2 && !service.refreshing },
                 10000, step1, "the opened popup builds its body")
  }
  function step1() {
    var view = panels[1].popup.bodyItem
    check(view !== null && view.service === service, "body built on open")
    check(panels[0].popup.bodyItem === null, "other monitors stay lazy")
    check(view && view.rowCount === 2, "two folder rows")
    if (view)
      tabKeys(view)
    if (view)
      statusInk(view)
    savedRow = view ? view.rowItem(0) : null
    check(savedRow !== null, "row delegate exists")
    check(view && view.listWheel && view.listWheel.target !== null, "the overview list scrolls with the kit wheel handling")
    // The other devices only; the fake core reports no Syncthing version.
    check(view && view.headerMeta === "1 / 2 devices online", "header without a version: " + (view && view.headerMeta))
    check(service.refresh(), "a refresh starts")
    // The fake core, like the real one, sends the snapshot before the result.
    wait.waitFor(function() { return !service.refreshing }, 10000, step2, "the refresh ends")
  }
  // The tab row is one cursor stop: ←/→ move between Folders and Devices,
  // Enter opens the tab under the cursor, Up and Down go between the row and
  // the rows, and a hovered tab takes the cursor. The key catcher's signals
  // stand in for the keys.
  function tabKeys(view) {
    var tabs = shell.find(view, "overviewTabs")
    var onTab = function(i) { return !!tabs && tabs.hasCursor && tabs.cursorIndex === i }
    var onFirstRow = function() { return !!view.rowItem(0) && view.rowItem(0).hasCursor }
    var count = function(i) {
      var label = shell.find(shell.find(tabs, "foamyTab" + i), "foamyTabCount")
      return label ? label.text : ""
    }
    check(count(0) === "2" && count(1) === "2", "the tabs count two folders and two devices: " + count(0) + " " + count(1))
    view.moveRequested(0, 1)
    check(onTab(0) && view.tab === "folders", "Down lands on the tab row, on the open Folders tab")
    view.moveRequested(0, 1)
    check(onFirstRow() && !onTab(0), "the next Down reaches the first folder row")
    view.moveRequested(0, -1)
    check(onTab(0), "Up comes back to the tab row, on the open tab")
    view.moveRequested(1, 0)
    check(onTab(1) && view.tab === "folders", "→ moves to Devices without opening it: " + view.tab)
    view.activateRequested()
    check(view.tab === "devices" && view.rowCount === 2, "Enter opens Devices: " + view.tab)
    view.moveRequested(-1, 0)
    view.activateRequested()
    check(onTab(0) && view.tab === "folders", "← and Enter open Folders again: " + view.tab)
    view.moveRequested(0, 1)
    if (tabs)
      tabs.hovered()
    check(onTab(0) && !onFirstRow(), "a hovered tab takes the cursor from the row")
  }
  // This scene runs in a dark theme with a dim red: the conflict's red
  // still reads where it is drawn, the hero title on the band and the
  // expanded row's state and detail on the row under the cursor, and the
  // row shows one red at rest and under the cursor.
  function statusInk(view) {
    var palette = Kit.FoamyPalette
    var ratio = function(c, under) { return palette.contrastRatio(c, under).toFixed(2) }
    check(palette.contrastRatio(palette.error, palette.background) < 4.5, "the theme's red is dim: " + palette.error)
    // The title starts on the wash's strongest stop. (On the band's
    // strongest point, that stop under the full glow, this red stays at
    // about 4.3:1: onSurface lightens it at most 1.7 times.)
    var title = shell.find(view, "heroTitle")
    check(title && title.text === "Needs attention" && palette.contrastRatio(title.color, palette.bandFill) >= 4.5,
          "the hero title reads on the band: " + (title && title.text) + " " + (title && title.color) + " at " + (title && ratio(title.color, palette.bandFill)) + ":1")
    var row = view.rowItem(0)
    check(row && row.row.state === "Conflicts", "the first row is the conflict: " + (row && row.row.state))
    if (!row)
      return
    view.toggleRow(row.key)
    var detail = shell.find(row, "rowDetail")
    var state = shell.find(row, "rowState")
    // As text: a copy, not a reference that follows the item.
    var rest = detail ? String(detail.color) : ""
    var restState = state ? String(state.color) : ""
    row.hovered()
    // What the row draws under the cursor, over the popup: its colour
    // animates to this fill.
    var under = Qt.tint(palette.background, row.fill)
    check(row.hasCursor && row.fill.a > 0, "the row takes the cursor: " + row.hasCursor + " " + row.fill)
    check(detail && detail.text.indexOf("sync conflict") >= 0 && palette.contrastRatio(detail.color, under) >= 4.5,
          "the conflict detail reads under the cursor: " + (detail && detail.color) + " at " + (detail && ratio(detail.color, under)) + ":1")
    check(state && palette.contrastRatio(state.color, under) >= 4.5,
          "the row state reads under the cursor: " + (state && state.color) + " at " + (state && ratio(state.color, under)) + ":1")
    check(detail && state && Qt.colorEqual(detail.color, state.color) && Qt.colorEqual(detail.color, rest) && Qt.colorEqual(state.color, restState),
          "one red in the row, at rest and under the cursor: " + rest + " " + restState + " " + (detail && detail.color) + " " + (state && state.color))
    view.toggleRow(row.key)
    // Give the cursor back to the tab row, where tabKeys left it.
    var tabs = shell.find(view, "overviewTabs")
    if (tabs)
      tabs.hovered()
  }
  function step2() {
    check(panels[1].popup.bodyItem.rowItem(0) === savedRow, "row delegate survives a snapshot (keyed model)")
    // FoamyPrefsQueue: an inline save shows at once in the view, before the host reflects it.
    panels[1].save("refreshIntervalSec", 300)
    check(panels[1].popup.bodyItem.settings.refreshIntervalSec === 300, "saved value shows before it is reflected")
    check(panels[1].bar.saved.length === 1 && panels[1].bar.saved[0].settings.refreshIntervalSec === 300, "saved inline through the bar")
    check(panels[1].bar.saved[0].settings.minimumConnectedDevices === 2 && panels[1].bar.saved[0].settings.probeIntervalSec === 5,
          "a save keeps the widget's other settings")
    // A shell shortcut (Ctrl+P) must not stop syncing; a plain p does. A
    // service action is marked running as soon as it is sent.
    var view = panels[1].popup.bodyItem
    view.textKey("p", Qt.ControlModifier)
    check(!service.serviceActionRunning, "Ctrl+P sends nothing")
    view.textKey("p", Qt.NoModifier)
    check(service.serviceActionRunning, "a plain p sends a service action")
    expect(function() { return !service.serviceActionRunning && shell.lifecycleActions().length >= 1 }, 10000, "the p key's request ends", function() {
      check(JSON.stringify(shell.lifecycleActions()) === JSON.stringify(["lifecycle.stop"]),
            "only the plain p stops the active service: " + JSON.stringify(shell.lifecycleActions()))
      movedFolder()
    })
  }
  // Refreshes with the same folders started no conflict walk; a moved
  // folder starts exactly one.
  function movedFolder() {
    // The refreshes and the p action have all ended by now.
    check(shell.scans === 0, "no conflict walk for unchanged folders (" + shell.scans + ")")
    var state = JSON.parse(snapshotFile.text())
    state.folders[1].path = state.folders[1].path.replace(/Photos$/, "Pictures")
    snapshotFile.setText(JSON.stringify(state))
    check(service.refresh(), "a refresh with a moved folder starts")
    expect(function() { return shell.scans >= 1 && !service.conflictScan.running }, 10000, "a moved folder starts a conflict walk", function() {
      check(shell.scans === 1, "one walk, none for the unchanged refreshes before (" + shell.scans + ")")
      check(JSON.stringify(service.conflictFolderIds) === JSON.stringify({ docs: true }), "conflicts after the walk: " + JSON.stringify(service.conflictFolderIds))
      panels[1].openSettings()
      expect(function() { return panels[1].editingSettings }, 10000, "settings open", step3)
    })
  }
  function step3() {
    panels[1].close()
    expect(function() { return !panels[1].opened && !panels[1].editingSettings }, 10000, "closing leaves settings", step4)
  }
  function step4() {
    // The settings IPC opens a closed popup straight into Settings.
    panels[2].openSettings()
    expect(function() { return panels[2].opened && panels[2].editingSettings }, 10000,
           "openSettings() on a closed popup shows Settings", step5)
  }
  function step5() {
    panels[2].close()
    // The folder chooser: the helper connects to a bus that never answers.
    panels[0].open()
    panels[0].browseFolder()
    expect(function() { return pickerLog.text().indexOf("connected foamy-syncthing") >= 0 }, 15000,
           "pick-folder reached the session bus", step6)
  }
  function step6() {
    check(panels[0].returningFromBrowser, "the chooser runs")
    panels[0].destroy()
    wait.waitFor(function() { return pickerLog.text().indexOf("closed") >= 0 && Kit.FoamyShared.memberCount("syncthing") === 2 }, 15000, step7)
  }
  function step7() {
    check(pickerLog.text().indexOf("closed") >= 0, "the chooser's helper went with its Panel: " + pickerLog.text())
    check(Kit.FoamyShared.memberCount("syncthing") === 2, "two members left")
    // panels[1] is now the first member, so the leader: its save must reach
    // the core as a configure request.
    panels[1].save("refreshIntervalSec", 120)
    expect(function() { return shell.configured(120) }, 10000, "the new leader's setting reaches the core", step8)
  }
  // The first item under `item` with this objectName.
  function find(item, name) {
    if (!item) return null
    if (item.objectName === name) return item
    for (var i = 0; i < item.children.length; i++) {
      var found = find(item.children[i], name)
      if (found) return found
    }
    return null
  }
  function step8() {
    // The chooser opens from the Add folder page.
    panels[1].openSettings()
    expect(function() { return panels[1].opened && panels[1].editingSettings && panels[1].popup.bodyItem !== null }, 10000, "settings open for the chooser", function() {
      var view = panels[1].popup.bodyItem
      view.openAdd("")
      check(view.addOpen, "the Add folder page opens")
      // The bus refuses this second chooser's authentication.
      view.browseRequested()
      // The bus logs "refused" after it answers, so wait for the log line too.
      expect(function() {
        return panels[1].opened && !panels[1].returningFromBrowser && panels[1].pickerError !== "" && pickerLog.text().indexOf("refused") >= 0
      }, 15000, "the refused chooser ends and the popup opens again", function() {
        var message = "The folder chooser needs a session bus, and there is none; enter the path manually."
        check(panels[1].pickerError === message, "chooser error: " + panels[1].pickerError)
        check(pickerLog.text().indexOf("refused") >= 0, "the chooser reached the refusing bus: " + pickerLog.text())
        // The offscreen popup has no window, so read the page state and the
        // error line under the path rather than effective visibility.
        var line = shell.find(view, "browseError")
        check(view.editingSettings && view.addOpen && line && line.text === message,
              "the Add folder page shows the chooser error: " + view.addOpen + " " + JSON.stringify(line && line.text))
        check(view.settingsError === "", "the Settings page does not: " + view.settingsError)
        // This scene runs in a dark theme with a dim red: the error line
        // still reads on the popup, as the kit's own error lines do.
        var palette = Kit.FoamyPalette
        check(palette.contrastRatio(palette.error, palette.background) < 4.5, "the theme's red is dim: " + palette.error)
        check(line && palette.contrastRatio(line.color, palette.background) >= 4.5,
              "the chooser error reads on the popup: " + (line && line.color) + " at "
              + (line && palette.contrastRatio(line.color, palette.background).toFixed(2)) + ":1")
        // A new attempt starts without the old error; the bus refuses it too.
        view.browseRequested()
        check(panels[1].pickerError === "" && line.text === "", "a new chooser clears the old error: " + JSON.stringify(line.text))
        expect(function() {
          return panels[1].opened && !panels[1].returningFromBrowser && panels[1].pickerError !== ""
        }, 15000, "the second refused chooser ends", function() {
          check(view.addOpen && line.text === message, "the error is back: " + JSON.stringify(line.text))
          // Back on Settings and in a new Add folder page the old error is gone.
          view.closeAdd()
          check(panels[1].pickerError === "", "leaving the Add folder page clears the chooser error: " + panels[1].pickerError)
          view.openAdd("")
          check(view.addOpen && line.text === "", "a new Add folder page starts without it: " + JSON.stringify(line.text))
          view.closeAdd()
          // The probe runs every 5 s while offline: a full interval online without one.
          wait.after(Math.max(0, 6000 - (Date.now() - shell.onlineAt)), function() {
            check(shell.probes() === shell.probesOnline, "install probe idle while online (" + shell.probesOnline + " -> " + shell.probes() + ")")
            console.log("FOAMY CHECKS " + checks)
            console.log("FOAMY DONE")
            Qt.quit()
          })
        })
      })
    })
  }
}
"""

CRASH_SHELL = r"""
import QtQuick
import Quickshell
import Quickshell.Io
import "harness"
import "plugin" as Plugin

ShellRoot {
  id: shell
  property var service: null
  property int checks: 0
  function check(ok, what) { checks++; if (!ok) console.log("FOAMY FAIL " + what) }
  function starts() { return coreLog.text().split("\n").filter(function(l) { return l.indexOf('"argv"') >= 0 }).length }
  FileView { id: coreLog; path: CORE_LOG; watchChanges: true; printErrors: false; onFileChanged: reload() }
  Component.onCompleted: {
    service = Qt.createComponent(Qt.resolvedUrl("plugin/Service.qml")).createObject(null)
    // The argv log reloads after the exit is seen, so wait for the count too.
    // The next start is 30 s away, so the count cannot overshoot.
    wait.waitFor(function() {
      return !service.core.running && service.core.restartTimer.running && service.core.restartTimer.interval === 30000 && shell.starts() >= 4
    }, 30000, slowRetry, "the crashing core reaches its slow retry")
  }
  FoamyWait { id: wait }
  property int before: 0
  function slowRetry() {
    var core = service.core
    // Counted from the fake core's argv log: the first start and three fast restarts.
    check(shell.starts() === 4, "three fast restarts, then a slow one (" + shell.starts() + " starts)")
    check(core.restartTimer.running && core.restartTimer.interval === 30000, "slow retry scheduled, not given up")
    check(JSON.stringify([1, 2, 3, 4, 5, 6, 7, 8, 9, 20].map(core.restartDelay))
            === JSON.stringify([250, 500, 1000, 30000, 60000, 120000, 240000, 300000, 300000, 300000]),
          "backoff doubles to a 5-minute cap: " + [1, 2, 3, 4, 5, 6, 7, 8, 9, 20].map(core.restartDelay))
    before = shell.starts()
    service.recover()
    wait.waitFor(function() { return shell.starts() > before }, 10000, function() {
      check(shell.starts() > before, "recover() starts the core right away")
      console.log("FOAMY CHECKS " + checks)
      console.log("FOAMY DONE")
      Qt.quit()
    })
  }
}
"""

# Syncthing missing, but a leftover user unit is in the way: the popup's
# summary names it, the bar tooltip (title only) does not.
BLOCKED_SHELL = r"""
import QtQuick
import Quickshell
import Quickshell.Io
import "harness"
import "plugin" as Plugin

ShellRoot {
  id: shell
  property var service: null
  property var panel: null
  property int checks: 0
  readonly property string blocking: Quickshell.env("FAKE_PROBE_BLOCKING")
  function check(ok, what) { checks++; if (!ok) console.log("FOAMY FAIL " + what) }
  FloatingWindow {
    implicitWidth: 400; implicitHeight: 300
    Item { id: host; width: 400; height: 300 }
  }
  Component { id: barComponent; MockBar { barSize: 34 } }
  FoamyWait { id: wait }
  Component.onCompleted: {
    service = Qt.createComponent(Qt.resolvedUrl("plugin/Service.qml")).createObject(null)
    panel = Qt.createComponent(Qt.resolvedUrl("plugin/Panel.qml")).createObject(host, {
      bar: barComponent.createObject(shell, { moduleName: "foamy.syncthing" }), moduleName: "foamy.syncthing", width: 40, height: 34 })
    wait.waitFor(function() { return !service.online && service.installationState === "incomplete" }, 20000, function(ok) {
      check(ok, "the probe reports an incomplete installation (" + service.installationState + ")")
      check(service.compactSummary.title === "Installation needs cleanup", "title: " + service.compactSummary.title)
      check(service.compactSummary.detail === blocking + " exists, but Syncthing isn't installed", "detail: " + service.compactSummary.detail)
      check(!service.canInstall, "Install stays disabled")
      // Pressing Install over the leftover unit starts nothing.
      service.installSyncthing()
      check(service.packageStatus === "" && !service.packageController.packageActionRunning, "Install over a leftover unit: " + service.packageStatus)
      check(panel.tooltip.indexOf(blocking) < 0 && panel.tooltip.indexOf("Installation needs cleanup") >= 0, "tooltip: " + panel.tooltip)
      console.log("FOAMY CHECKS " + checks)
      console.log("FOAMY DONE")
      Qt.quit()
    })
  }
}
"""

# The first installation check hangs: it is stopped at its deadline and
# reported as a failure, which stays while the next check runs and goes when
# that check succeeds.
HUNG_PROBE_SHELL = r"""
import QtQuick
import Quickshell
import Quickshell.Io
import "harness"
import "plugin" as Plugin

ShellRoot {
  id: shell
  property var service: null
  property int checks: 0
  property var errors: []
  // The failure went while a check ran: the popup never showed it.
  property bool erasedWhileChecking: false
  function check(ok, what) { checks++; if (!ok) console.log("FOAMY FAIL " + what) }
  function probes() { return probeLog.text().split("\n").filter(function(l) { return l === "status" }).length }
  FileView { id: probeLog; path: PROBE_LOG; watchChanges: true; printErrors: false; onFileChanged: reload() }
  FileView { id: hangPid; path: HANG_PID; blockLoading: true; printErrors: false }
  FileView { id: childStat; blockLoading: true; printErrors: false }
  // The hung check's child has ended (gone, or a zombie not yet reaped).
  function childGone() {
    hangPid.reload()
    var pid = hangPid.text().trim()
    if (!/^[0-9]+$/.test(pid)) return false
    childStat.path = "/proc/" + pid + "/stat"
    childStat.reload()
    var stat = childStat.text()
    return stat === "" || stat.slice(stat.lastIndexOf(")") + 2).charAt(0) === "Z"
  }
  Connections {
    target: shell.service ? shell.service.packageController : null
    function onPackageErrorChanged() {
      var controller = shell.service.packageController
      if (controller.packageError)
        shell.errors = shell.errors.concat([controller.packageError])
      else if (shell.errors.length > 0 && controller.refreshing)
        shell.erasedWhileChecking = true
    }
  }
  FoamyWait { id: wait }
  Component.onCompleted: {
    service = Qt.createComponent(Qt.resolvedUrl("plugin/Service.qml")).createObject(null)
    var controller = service.packageController
    wait.waitFor(function() { return shell.probes() === 1 && controller.refreshing }, 20000, function(ok) {
      check(ok, "the first installation check runs")
      // A check asked for while one runs waits for it.
      controller.updateStatus()
      wait.waitFor(function() { return shell.probes() >= 2 && !controller.refreshing && controller.state === "existing" }, 30000, function(ok) {
        check(ok, "a later installation check runs (" + shell.probes() + " checks, state " + controller.state + ")")
        check(shell.errors.length >= 1 && shell.errors[0] === "Could not check Syncthing installation",
              "the stopped check is reported as a failure: " + JSON.stringify(shell.errors))
        check(!shell.erasedWhileChecking, "the failure stays while the next check runs")
        check(controller.packageError === "", "a check that succeeds clears the failure: " + controller.packageError)
        wait.waitFor(shell.childGone, 5000, function(ok) {
          check(ok, "the hung check's child process is stopped with it")
          console.log("FOAMY CHECKS " + checks)
          console.log("FOAMY DONE")
          Qt.quit()
        })
      })
    })
  }
}
"""

# The first installation check hangs and the service goes while it runs (a
# shell reload): the check's processes go with it.
HUNG_PROBE_TEARDOWN_SHELL = r"""
import QtQuick
import Quickshell
import Quickshell.Io
import "harness"
import "plugin" as Plugin

ShellRoot {
  id: shell
  property var service: null
  property int checks: 0
  function check(ok, what) { checks++; if (!ok) console.log("FOAMY FAIL " + what) }
  FileView { id: hangPid; path: HANG_PID; blockLoading: true; printErrors: false }
  FileView { id: childStat; blockLoading: true; printErrors: false }
  // The hung check's child, once it runs: its pid, else "".
  function childPid() {
    hangPid.reload()
    var pid = hangPid.text().trim()
    return /^[0-9]+$/.test(pid) ? pid : ""
  }
  // The hung check's child has ended (gone, or a zombie not yet reaped).
  function childGone() {
    var pid = childPid()
    if (!pid) return false
    childStat.path = "/proc/" + pid + "/stat"
    childStat.reload()
    var stat = childStat.text()
    return stat === "" || stat.slice(stat.lastIndexOf(")") + 2).charAt(0) === "Z"
  }
  FoamyWait { id: wait }
  Component.onCompleted: {
    service = Qt.createComponent(Qt.resolvedUrl("plugin/Service.qml")).createObject(null)
    wait.waitFor(function() { return shell.childPid() !== "" && service.packageController.refreshing }, 20000, function(ok) {
      check(ok, "the installation check hangs in a child process")
      service.destroy()
      wait.waitFor(shell.childGone, 5000, function(ok) {
        check(ok, "the hung check's child goes with the service")
        console.log("FOAMY CHECKS " + checks)
        console.log("FOAMY DONE")
        Qt.quit()
      })
    })
  }
}
"""

# Folder actions, start/stop, the bar colour and protocol failures through the
# real Service and one Panel, against the fake core's request log. The fake
# core fails or holds some actions (SERVICE_FAILURES); the scene rewrites
# SNAPSHOT to change what the next refresh publishes.
SERVICE_SHELL = r"""
import QtQuick
import Quickshell
import Quickshell.Io
import "harness"
import "plugin/kit" as Kit
import "plugin" as Plugin

ShellRoot {
  id: shell
  property var service: null
  property var panel: null
  property int checks: 0
  // Actions already checked; since() is what came after them.
  property int mark: 0
  property int busyEnds: 0
  function check(ok, what) { checks++; if (!ok) console.log("FOAMY FAIL " + what) }
  function expect(condition, ms, what, next) { wait.waitFor(condition, ms, function(ok) { check(ok, what); next() }) }
  function requests() {
    return coreLog.text().split("\n").filter(function(l) { return l.indexOf('{"request"') === 0 }).map(function(l) { return JSON.parse(l).request })
  }
  // The actions the core received, without the scene's own fake.* ones.
  function actions() {
    return requests().filter(function(r) { return r.type === "action" && r.action.indexOf("fake.") !== 0 })
      .map(function(r) { return r.action + " " + JSON.stringify(r.args) })
  }
  function since() { return actions().slice(mark) }
  // Checks that exactly `expected` came since the last step, then moves the mark.
  function sent(expected, what) {
    var got = since()
    check(JSON.stringify(got) === JSON.stringify(expected), what + ": " + JSON.stringify(got))
    mark += expected.length
  }
  function rewrite(change) {
    var state = JSON.parse(snapshotFile.text())
    change(state)
    snapshotFile.setText(JSON.stringify(state))
  }
  function needs(items) {
    rewrite(function(state) { state.folders[0].status.needTotalItems = items })
  }
  function problems(count) {
    rewrite(function(state) { state.counts.folderProblems = count })
  }
  function colour(value) { return Qt.colorEqual(panel.iconColor, value) }
  function probes() { return probeLog.text().split("\n").filter(function(l) { return l === "status" }).length }

  FloatingWindow {
    implicitWidth: 400; implicitHeight: 300
    Item { id: host; width: 400; height: 300 }
  }
  Component { id: barComponent; MockBar { barSize: 34 } }
  FileView { id: coreLog; path: CORE_LOG; watchChanges: true; printErrors: false; onFileChanged: reload() }
  FileView { id: probeLog; path: PROBE_LOG; watchChanges: true; printErrors: false; onFileChanged: reload() }
  FileView { id: snapshotFile; path: SNAPSHOT; blockLoading: true; blockWrites: true; printErrors: false }
  Connections {
    target: shell.service
    function onFolderMutationBusyChanged() { if (!shell.service.folderMutationBusy) shell.busyEnds++ }
  }
  FoamyWait { id: wait }

  Component.onCompleted: {
    service = Qt.createComponent(Qt.resolvedUrl("plugin/Service.qml")).createObject(null)
    panel = Qt.createComponent(Qt.resolvedUrl("plugin/Panel.qml")).createObject(host, {
      bar: barComponent.createObject(shell, { moduleName: "foamy.syncthing" }), moduleName: "foamy.syncthing", width: 40, height: 34,
      settings: { minimumConnectedDevices: 0 } })
    // The installation check that runs at start, before Syncthing is
    // online, fails (probe_fail).
    wait.waitFor(function() {
      return service.online && service.settingsReady && service.folderRows.length === 5
        && service.scannedFolderKey === service.folderKey && !service.conflictScan.running && service.conflictInput === ""
        && shell.probes() >= 1 && !service.packageController.refreshing
    }, 20000, function(ok) {
      check(ok, "the service comes online with five folders")
      check(colour(panel.bar.foreground) && !service.tooFewDevices, "a healthy bar uses the bar colour")
      check(service.packageController.packageError === "status check failed",
            "the first installation check failed: " + service.packageController.packageError)
      check(service.packageError === "", "the failed check says nothing while Syncthing is online: " + service.packageError)
      pause()
    })
  }

  function pause() {
    check(service.setFolderPaused("docs", true), "pause starts")
    expect(function() { return !service.folderMutationBusy && since().length >= 1 }, 10000, "the pause ends", function() {
      sent(['folder.pause {"folderId":"docs"}'], "pause sends only the folder id")
      check(service.folderMutationNotice === "Paused Documents. Folder configuration, sharing, and local files are unchanged.",
            "pause notice: " + service.folderMutationNotice)
      check(service.setFolderPaused("docs", false), "resume starts")
      expect(function() { return !service.folderMutationBusy && since().length >= 1 }, 10000, "the resume ends", function() {
        sent(['folder.resume {"folderId":"docs"}'], "resume sends only the folder id")
        check(service.folderMutationNotice === "Resumed Documents. Folder configuration, sharing, and local files are unchanged.",
              "resume notice: " + service.folderMutationNotice)
        unlink()
      })
    })
  }

  // An active folder is paused first, and stays busy until its removal ends.
  function unlink() {
    var ends = busyEnds
    check(service.unlinkFolder("docs"), "unlink starts")
    check(service.folderMutationBusy, "unlink is busy at once")
    check(!service.unlinkFolder("archive") && service.folderMutationError === "Another folder operation is already running",
          "a second unlink is refused while one runs: " + service.folderMutationError)
    expect(function() { return !service.folderMutationBusy && since().length >= 2 }, 10000, "the unlink ends", function() {
      sent(['folder.pause {"folderId":"docs"}', 'folder.forget {"folderId":"docs"}'], "unlink pauses, then removes")
      check(busyEnds === ends + 1, "busy from the pause to the removal (" + (busyEnds - ends) + " ends)")
      check(service.folderMutationNotice === "Unlinked Documents from Syncthing. Local files were kept.",
            "unlink notice: " + service.folderMutationNotice)
      // The refused second unlink's message does not outlive the first one.
      check(service.folderMutationError === "", "no error next to the notice: " + service.folderMutationError)
      check(service.unlinkFolder("archive"), "unlinking a paused folder starts")
      expect(function() { return !service.folderMutationBusy && since().length >= 1 }, 10000, "the paused unlink ends", function() {
        sent(['folder.forget {"folderId":"archive"}'], "a paused folder is removed without a pause")
        failedPause()
      })
    })
  }

  function failedPause() {
    check(service.unlinkFolder("fail-pause"), "unlink with a failing pause starts")
    expect(function() { return !service.folderMutationBusy && since().length >= 1 }, 10000, "the failed pause ends", function() {
      check(service.folderMutationError === "Cannot pause", "pause error: " + service.folderMutationError)
      sent(['folder.pause {"folderId":"fail-pause"}'], "nothing is removed after a failed pause")
      check(service.unlinkFolder("fail-forget"), "unlink with a failing removal starts")
      expect(function() { return !service.folderMutationBusy && since().length >= 2 }, 10000, "the failed removal ends", function() {
        check(service.folderMutationError === "Cannot remove The folder remains paused; its local files are unchanged.",
              "removal error: " + service.folderMutationError)
        sent(['folder.pause {"folderId":"fail-forget"}', 'folder.forget {"folderId":"fail-forget"}'], "failing removal")
        check(!service.unlinkFolder("missing-id") && service.folderMutationError === "The selected folder is no longer configured",
              "an unknown folder is refused: " + service.folderMutationError)
        lifecycle()
      })
    })
  }

  // A failed stop settles; the unknown folder above sent nothing before it.
  function lifecycle() {
    check(service.toggleService(), "stop starts")
    expect(function() { return !service.serviceActionRunning && since().length >= 1 }, 10000, "the stop ends", function() {
      sent(["lifecycle.stop {}"], "stop is the only request")
      check(service.controlError === "authorized user-service action failed", "stop error: " + service.controlError)
      outOfSync()
    })
  }

  function outOfSync() {
    // An idle folder with outstanding items is a problem although Syncthing counts none.
    needs(2)
    check(service.refresh(), "a refresh starts")
    expect(function() { return colour(panel.bar.urgent) && !service.refreshing }, 10000, "an out-of-sync folder makes the bar urgent", function() {
      check(panel.barIcon.codePointAt(0) === 0xf09e0, "notify glyph: " + panel.barIcon.codePointAt(0))
      needs(0)
      check(service.refresh(), "a second refresh starts")
      expect(function() { return colour(panel.bar.foreground) && !service.refreshing }, 10000, "an up-to-date folder clears it", function() {
        check(panel.barIcon.codePointAt(0) === 0xf0160, "default glyph: " + panel.barIcon.codePointAt(0))
        folderProblems()
      })
    })
  }

  // A problem Syncthing counts is urgent too, ahead of the too-few-devices warning.
  function folderProblems() {
    problems(1)
    check(service.refresh(), "a refresh starts")
    expect(function() { return colour(panel.bar.urgent) && !service.refreshing }, 10000, "a counted folder problem makes the bar urgent", function() {
      recheckDuringUnlink()
    })
  }

  // A recheck that fails while an unlink runs keeps its message after the
  // unlink succeeds: the core answers it between the pause and the removal.
  function recheckDuringUnlink() {
    check(service.unlinkFolder("docs"), "an unlink starts before a recheck")
    check(service.refresh(true), "a recheck starts while the unlink runs")
    expect(function() { return !service.folderMutationBusy && !service.refreshing && since().length >= 3 }, 10000, "the unlink and the recheck end", function() {
      sent(['folder.pause {"folderId":"docs"}', "folder.recheck-errors {}", 'folder.forget {"folderId":"docs"}'], "the recheck goes between pause and removal")
      check(service.folderMutationError === "Cannot recheck", "the failed recheck's message stays: " + service.folderMutationError)
      addDuringRecheck()
    })
  }

  // The first item under `item` with this objectName.
  function find(item, name) {
    if (!item) return null
    if (item.objectName === name) return item
    for (var i = 0; i < item.children.length; i++) {
      var found = find(item.children[i], name)
      if (found) return found
    }
    return null
  }

  // A recheck (a right-click on the bar) still running when Add folder is
  // pressed fails first, as the core answers in order; the add then
  // succeeds and its page closes.
  function addDuringRecheck() {
    panel.openSettings()
    expect(function() { return panel.opened && panel.editingSettings && panel.view !== null && !service.refreshing }, 10000, "Settings open", function() {
      var view = panel.view
      view.openAdd("")
      view.setAddPath("/home/me/Music")
      expect(function() { return service.folderIdSuggestion !== "" && !service.folderPreparationBusy && !service.refreshing }, 10000, "a folder ID is suggested", function() {
        check(service.refresh(true), "a recheck starts")
        shell.find(view, "submitButton").clicked()
        check(service.folderMutationBusy && service.folderMutationAction === "add", "the add starts while the recheck runs")
        expect(function() { return !service.folderMutationBusy && !service.refreshing && since().length >= 3 }, 10000, "the recheck and the add end", function() {
          sent(["folder.suggest-id {}", "folder.recheck-errors {}",
                'folder.add-existing {"folderId":"abcde-fghij","path":"/home/me/Music","createDirectory":false,"label":"Music","deviceIds":[],"pendingDeviceId":""}'],
               "the recheck is answered before the add")
          check(service.folderMutationError === "Cannot recheck", "the recheck's message: " + service.folderMutationError)
          check(service.folderMutationNotice === "Local only — this folder will not sync with other devices.", "the add's notice: " + service.folderMutationNotice)
          check(!view.addOpen, "the Add folder page closes after the add succeeds")
          panel.close()
          expect(function() { return !panel.opened && !service.refreshing }, 10000, "the popup closes", tooFewDevices)
        })
      })
    })
  }

  function tooFewDevices() {
    // Two connected with this computer: three is too few.
    panel.save("minimumConnectedDevices", 3)
    expect(function() { return service.tooFewDevices }, 10000, "the saved minimum reaches the service", function() {
      check(colour(panel.bar.urgent), "a folder problem stays urgent with too few devices")
      problems(0)
      check(service.refresh(), "a refresh starts")
      expect(function() { return colour(Kit.FoamyPalette.warning) && !service.refreshing }, 10000, "too few devices is a warning", function() {
        protocol(0, -1)
      })
    })
  }

  readonly property var badLines: ['not json', '{"v":99,"type":"snapshot"}', '{"v":2,"type":"snapshot","revision":1,"state":{}}',
    '{"v":2,"type":"result","id":"999999","ok":true}', '{"v":2,"type":"fatal","code":"x","message":"boom"}', '{"v":2,"type":"bogus"}']
  // Online with no restart under way.
  function settled() { return service.online && service.core.desiredRunning && !service.core.restartTimer.running }
  // Each bad line stops the core once: the pending request fails once, nothing
  // restarts by itself, and recover() starts it again, exactly once.
  function protocol(index, generation) {
    if (index >= badLines.length) {
      coreExit(generation)
      return
    }
    var line = badLines[index]
    expect(function() { return settled() && service.core.generation > generation }, 10000, "the core runs before " + line, function() {
      var calls = 0, late = 0, error = null
      var running = service.core.generation
      if (index > 0)
        check(running === generation + 1, "recover() starts one core before " + line + " (" + (running - generation) + " starts)")
      service.core.action("fake.hold", {}, function(ok, data, failure) { calls++; error = failure })
      service.core.action("fake.emit", { line: line }, null)
      expect(function() {
        return service.core.incompatible && service.phase === "core-incompatible" && calls === 1 && error && error.code === "core_unavailable" && !service.core.running
      }, 10000, "the core stops on " + line, function() {
        wait.after(500, function() {
          check(calls === 1, "the pending request failed once (" + calls + ")")
          check(!service.core.running && !service.core.restartTimer.running && service.core.generation === running, "no restart by itself after " + line)
          check(service.core.refresh(function() { late++ }) === "" && late === 0, "no request while stopped")
          service.recover()
          protocol(index + 1, running)
        })
      })
    })
  }

  // A core that dies ends the folder action it holds and comes back by itself.
  function coreExit(generation) {
    expect(function() { return settled() && service.core.generation > generation }, 10000, "recover() starts the core again", function() {
      var running = service.core.generation
      check(running === generation + 1, "recover() starts one core (" + (running - generation) + " starts)")
      check(service.setFolderPaused("hold-me", true) && service.folderMutationBusy, "a held pause is busy")
      service.core.action("fake.exit", {}, null)
      expect(function() { return !service.folderMutationBusy && service.folderMutationError !== "" }, 10000, "the held pause fails with the core", function() {
        expect(function() { return settled() && service.core.generation > running && since().length >= 1 }, 5000, "the core restarts at once", function() {
          sent(['folder.pause {"folderId":"hold-me"}'], "the held pause")
          offline()
        })
      })
    })
  }

  // The next refresh publishes Syncthing down or up.
  function reachable(online) {
    rewrite(function(state) {
      state.connection = online ? { online: true, phase: "online", fresh: true }
        : { online: false, phase: "error", error: { code: "connection", message: "could not connect to Syncthing" } }
    })
    check(service.refresh(), "a refresh starts")
  }

  // Syncthing down is urgent, ahead of the too-few-devices warning.
  function offline() {
    reachable(false)
    expect(function() { return service.phase === "error" && colour(panel.bar.urgent) && !service.refreshing }, 10000, "Syncthing down makes the bar urgent", function() {
      check(panel.barIcon.codePointAt(0) === 0xf09e0, "notify glyph while down: " + panel.barIcon.codePointAt(0))
      // Folder changes are refused before a request while Syncthing is down.
      check(!service.unlinkFolder("docs") && service.folderMutationError === "Syncthing must be online to unlink a folder",
            "unlink is refused offline: " + service.folderMutationError)
      check(!service.setFolderPaused("docs", true) && service.folderMutationError === "Syncthing must be online to manage folders",
            "pause is refused offline: " + service.folderMutationError)
      check(actions().length === mark, "no other action: " + JSON.stringify(since()))
      // A refusal's message goes when Syncthing is back.
      reachable(true)
      expect(function() { return service.online && !service.refreshing }, 10000, "Syncthing is back", function() {
        check(service.folderMutationError === "", "the refused pause's message goes with the outage: " + service.folderMutationError)
        reachable(false)
        expect(function() { return !service.online && !service.refreshing }, 10000, "Syncthing goes down again", function() {
          check(!service.unlinkFolder("docs") && service.folderMutationError === "Syncthing must be online to unlink a folder",
                "unlink is refused offline again: " + service.folderMutationError)
          reachable(true)
          expect(function() { return service.online && !service.refreshing }, 10000, "Syncthing is back again", function() {
            check(service.folderMutationError === "", "the refused unlink's message goes with the outage: " + service.folderMutationError)
            check(actions().length === mark, "no other action: " + JSON.stringify(since()))
            console.log("FOAMY CHECKS " + checks)
            console.log("FOAMY DONE")
            Qt.quit()
          })
        })
      })
    })
  }
}
"""

SERVICE_FAILURES = {
    "folder.pause fail-pause": {"code": "offline", "message": "Cannot pause"},
    "folder.forget fail-forget": {"code": "folder_changed", "message": "Cannot remove"},
    "folder.pause hold-me": "hold",
    "folder.recheck-errors": {"code": "offline", "message": "Cannot recheck"},
    "lifecycle.stop": {"code": "lifecycle_failed", "message": "authorized user-service action failed"},
}


def service_snapshot(work):
    """Five folders whose paths do not exist (the real conflict scan finds
    none): `docs` first, `archive` paused."""

    def folder(folder_id, label="", paused=False):
        return {
            "id": folder_id,
            "label": label,
            "path": str(work / "absent" / folder_id),
            "paused": paused,
            "devices": [{"id": "PEER1"}],
            "status": {"state": "idle", "needTotalItems": 0, "globalFiles": 1, "globalBytes": 10},
        }

    return {
        "connection": {"online": True, "phase": "online", "fresh": True},
        "identity": {"deviceId": "LOCAL"},
        "lifecycle": {
            "active": True,
            "canControl": True,
            "targetMatch": True,
            "activeState": "active",
            "unitFileState": "enabled",
        },
        "webUi": {"url": "http://127.0.0.1:8384"},
        "counts": {"folders": 5, "devices": 1, "connectedDevices": 1, "folderProblems": 0},
        "devices": [
            {"id": "LOCAL", "name": "this", "connected": True},
            {"id": "PEER1", "name": "laptop", "connected": True},
        ],
        "folders": [
            folder("docs", "Documents"),
            folder("archive", "Archive", paused=True),
            folder("fail-pause"),
            folder("fail-forget"),
            folder("hold-me"),
        ],
        "pendingFolders": {},
    }


# No helper for this CPU: the core and the folder chooser say so once, and
# nothing is retried.
MISSING_SHELL = r"""
import QtQuick
import Quickshell
import "harness"
import "plugin" as Plugin
import "plugin/kit/FoamyNative.js" as FoamyNative

ShellRoot {
  id: shell
  property var service: null
  property var panel: null
  property int checks: 0
  function check(ok, what) { checks++; if (!ok) console.log("FOAMY FAIL " + what) }
  FloatingWindow {
    implicitWidth: 400; implicitHeight: 300
    Item { id: host; width: 400; height: 300 }
  }
  Component { id: barComponent; MockBar { barSize: 34 } }
  FoamyWait { id: wait }
  Component.onCompleted: {
    service = Qt.createComponent(Qt.resolvedUrl("plugin/Service.qml")).createObject(null)
    panel = Qt.createComponent(Qt.resolvedUrl("plugin/Panel.qml")).createObject(host, {
      bar: barComponent.createObject(shell, { moduleName: "foamy.syncthing" }), moduleName: "foamy.syncthing", width: 40, height: 34 })
    wait.waitFor(function() { return !service.core.executableReady }, 20000, function(ok) {
      check(ok, "the missing helper is noticed")
      check(service.core.unavailable && service.lastError === FoamyNative.UNAVAILABLE_TEXT, "core error: " + service.lastError)
      // A relaunch of a missing helper fails within milliseconds and leaves
      // the same state, so count launches instead of reading the state.
      var launches = service.core.generation
      service.recover()
      wait.after(500, function() {
        check(service.core.generation === launches, "recover() launches nothing (" + (service.core.generation - launches) + ")")
        check(!service.core.running && !service.core.restartTimer.running, "recover() does not retry a missing helper")
        panel.open()
        panel.browseFolder()
        wait.waitFor(function() { return panel.opened && panel.pickerError !== "" && !panel.returningFromBrowser }, 15000, function(ok) {
          check(ok, "the chooser ends and the popup opens again")
          check(panel.pickerError === FoamyNative.UNAVAILABLE_TEXT, "chooser error: " + panel.pickerError)
          check(!service.core.running && service.core.generation === launches,
                "the core is still not started (" + (service.core.generation - launches) + " launches)")
          console.log("FOAMY CHECKS " + checks)
          console.log("FOAMY DONE")
          Qt.quit()
        })
      })
    })
  }
}
"""

# The folder chooser answers a path (FAKE_PICKER_PATH) for the Add folder
# page, in a window so keys reach the field: the caret goes after the path,
# so typing extends it, and a path wider than the field shows its end.
CHOOSER_SHELL = r"""
import QtQuick
import QtTest
import Quickshell
import "harness"
import "plugin" as Plugin

ShellRoot {
  id: shell
  property var service: null
  property var panel: null
  property int checks: 0
  readonly property string chosen: Quickshell.env("FAKE_PICKER_PATH")
  function check(ok, what) { checks++; if (!ok) console.log("FOAMY FAIL " + what) }
  // The first item under `item` with this objectName.
  function find(item, name) {
    if (!item) return null
    if (item.objectName === name) return item
    for (var i = 0; i < item.children.length; i++) {
      var found = find(item.children[i], name)
      if (found) return found
    }
    return null
  }
  FloatingWindow {
    implicitWidth: 1920; implicitHeight: 1080
    // The popup draws into this screen's overlay, so it has a window.
    ScreenSim { id: sim }
    Item {
      id: host
      x: 1500; width: 40; height: 34
      TestEvent { id: keys }
    }
  }
  Component { id: barComponent; MockBar { barSize: 34 } }
  FoamyWait { id: wait }
  Component.onCompleted: {
    service = Qt.createComponent(Qt.resolvedUrl("plugin/Service.qml")).createObject(null)
    panel = Qt.createComponent(Qt.resolvedUrl("plugin/Panel.qml")).createObject(host, {
      bar: barComponent.createObject(shell, { moduleName: "foamy.syncthing" }), moduleName: "foamy.syncthing", width: 40, height: 34 })
    wait.waitFor(function() { return service.online }, 20000, function(ok) {
      check(ok, "the service comes online")
      panel.openSettings()
      wait.waitFor(function() { return panel.opened && panel.editingSettings && panel.view !== null }, 10000, function(ok) {
        check(ok, "Settings open")
        panel.view.openAdd("")
        var field = shell.find(panel.view, "pathField")
        // Laid out and focused, as when the chooser's button on it is pressed.
        wait.waitFor(function() { return field && field.width > 0 && field.inputActiveFocus }, 10000, function(ok) {
          check(ok, "the Add folder page shows its path field")
          panel.browseFolder()
          wait.waitFor(function() {
            return panel.opened && !panel.returningFromBrowser && field.text === shell.chosen && field.inputActiveFocus
          }, 15000, function(ok) {
            check(ok, "the chosen path fills the field, which has the keys: " + JSON.stringify(field.text))
            check(field.contentWidth > field.width, "the path is wider than the field")
            var end = field.editorItem.positionToRectangle(field.text.length).x
            check(end >= field.contentX && end <= field.contentX + field.width,
                  "the end of the path is in view: it is at " + end + ", the view from " + field.contentX + " to " + (field.contentX + field.width))
            keys.keyClick(Qt.Key_X, Qt.NoModifier, -1)
            check(field.text === shell.chosen + "x", "typing extends the chosen path: " + JSON.stringify(field.text))
            console.log("FOAMY CHECKS " + checks)
            console.log("FOAMY DONE")
            Qt.quit()
          })
        })
      })
    })
  }
}
"""

# Settings in a window that takes keys, for a widget without a bar entry (no
# moduleName). Closed on Settings and opened again, the popup shows the
# overview and a real Esc closes it; after an Esc that leaves Settings, the
# next Esc closes it too. The settings queue refuses every save, and a pick
# in each settings choice shows the saved value again at once, beside the
# error.
SETTINGS_SHELL = r"""
import QtQuick
import QtTest
import Quickshell
import "harness"
import "plugin" as Plugin

ShellRoot {
  id: shell
  property var service: null
  property var panel: null
  property var bar: null
  property int checks: 0
  function check(ok, what) { checks++; if (!ok) console.log("FOAMY FAIL " + what) }
  // The first item under `item` with this objectName.
  function find(item, name) {
    if (!item) return null
    if (item.objectName === name) return item
    for (var i = 0; i < item.children.length; i++) {
      var found = find(item.children[i], name)
      if (found) return found
    }
    return null
  }
  FloatingWindow {
    implicitWidth: 1920; implicitHeight: 1080
    // The popup draws into this screen's overlay, so it has a window and
    // takes the keys sent to it.
    ScreenSim { id: sim }
    Item {
      id: host
      x: 1500; width: 40; height: 34
      TestEvent { id: keys }
    }
  }
  Component { id: barComponent; MockBar { barSize: 34 } }
  FoamyWait { id: wait }
  Component.onCompleted: {
    service = Qt.createComponent(Qt.resolvedUrl("plugin/Service.qml")).createObject(null)
    bar = barComponent.createObject(shell, { moduleName: "foamy.syncthing" })
    panel = Qt.createComponent(Qt.resolvedUrl("plugin/Panel.qml")).createObject(host, { bar: bar, width: 40, height: 34 })
    wait.waitFor(function() { return service.online }, 20000, function(ok) {
      check(ok, "the service comes online")
      settingsOpen(closedOnSettings)
    })
  }
  // Opens the popup on Settings, then `next`.
  function settingsOpen(next) {
    panel.openSettings()
    wait.waitFor(function() { return panel.opened && panel.editingSettings && panel.view !== null }, 10000, function(ok) {
      check(ok, "Settings open")
      next()
    })
  }
  function closedOnSettings() {
    panel.close()
    wait.waitFor(function() { return !panel.opened && !panel.editingSettings }, 10000, function(ok) {
      check(ok, "closing the popup leaves Settings")
      panel.open()
      wait.waitFor(function() { return panel.opened && panel.view.visible }, 10000, function(ok) {
        check(ok, "the popup opens again")
        keys.keyClick(Qt.Key_Escape, Qt.NoModifier, -1)
        wait.waitFor(function() { return !panel.opened }, 5000, function(ok) {
          check(ok, "Esc closes the popup that was closed on Settings")
          settingsOpen(leftWithEsc)
        })
      })
    })
  }
  function leftWithEsc() {
    keys.keyClick(Qt.Key_Escape, Qt.NoModifier, -1)
    wait.waitFor(function() { return !panel.editingSettings }, 5000, function(ok) {
      check(ok && panel.opened, "Esc on Settings goes back to the overview")
      keys.keyClick(Qt.Key_Escape, Qt.NoModifier, -1)
      wait.waitFor(function() { return !panel.opened }, 5000, function(ok) {
        check(ok, "the next Esc closes the popup")
        settingsOpen(function() {
          refusedPicks()
          console.log("FOAMY CHECKS " + checks)
          console.log("FOAMY DONE")
          Qt.quit()
        })
      })
    })
  }
  // A pick, as the dropdown's list makes it, of a value other than the saved one.
  function refusedPicks() {
    var picks = [["refreshDropdown", "900"], ["openRefreshDropdown", "30"], ["minimumDevicesDropdown", "3"]]
    for (var i = 0; i < picks.length; i++) {
      var dropdown = find(panel.view, picks[i][0])
      var saved = dropdown ? dropdown.value : ""
      if (dropdown)
        dropdown.select(picks[i][1])
      check(dropdown && saved !== picks[i][1] && dropdown.shownValue === saved,
            picks[i][0] + ": a refused pick of " + picks[i][1] + " shows the saved value again at once (shows "
            + (dropdown ? dropdown.shownValue : "no dropdown") + ", saved " + saved + ")")
    }
    check(panel.view.settingsError === "Settings can't be saved: this widget has no bar entry.",
          "the refusal shows: " + JSON.stringify(panel.view.settingsError))
    check(bar.saved.length === 0, "nothing is saved: " + JSON.stringify(bar.saved))
  }
}
"""

# The real core against tests/fake_syncthing.py: the header's version and
# device line, and each peer's version, end to end (Syncthing → helper →
# Service → view). The fake reports v2.0.10 for this device and v2.0.9 for
# the connected peer.
LIVE_SHELL = r"""
import QtQuick
import Quickshell
import "harness"
import "plugin" as Plugin

ShellRoot {
  id: shell
  property var service: null
  property var panel: null
  property int checks: 0
  function check(ok, what) { checks++; if (!ok) console.log("FOAMY FAIL " + what) }
  function device(view, name) {
    var devices = view && view.liveRows ? view.liveRows.devices : []
    for (var i = 0; i < devices.length; i++)
      if (devices[i].label === name) return devices[i]
    return null
  }
  FloatingWindow {
    implicitWidth: 400; implicitHeight: 300
    Item { id: host; width: 400; height: 300 }
  }
  Component { id: barComponent; MockBar { barSize: 34 } }
  FoamyWait { id: wait }
  Component.onCompleted: {
    service = Qt.createComponent(Qt.resolvedUrl("plugin/Service.qml")).createObject(null)
    panel = Qt.createComponent(Qt.resolvedUrl("plugin/Panel.qml")).createObject(host, {
      bar: barComponent.createObject(shell, { moduleName: "foamy.syncthing" }), moduleName: "foamy.syncthing", width: 40, height: 34 })
    wait.waitFor(function() { return service.online && service.folderRows.length === 1 && service.devices.length === 3 }, 30000, function(ok) {
      check(ok, "the real core brings the fake Syncthing online (" + service.phase + " " + service.lastError + ")")
      panel.open()
      wait.waitFor(function() { return panel.popup.bodyItem !== null && shell.device(panel.popup.bodyItem, "laptop") !== null }, 10000, function(ok) {
        var view = panel.popup.bodyItem
        check(ok, "the popup shows the devices")
        check(service.syncthingVersion === "v2.0.10", "our version: '" + service.syncthingVersion + "'")
        // The other devices only, like the Devices tab: laptop online, phone not.
        check(view.headerMeta === "1 / 2 devices online · v2.0.10", "header meta: " + view.headerMeta)
        check(view.summary.title === "Up to date" && view.summary.detail === "", "no folder count under Up to date: " + JSON.stringify(view.summary))
        check(view.errorText === "", "no error: " + view.errorText)
        var laptop = shell.device(view, "laptop")
        check(laptop && laptop.connected && laptop.subtitle === "1 shared folder · v2.0.9", "connected peer: " + (laptop && laptop.subtitle))
        var phone = shell.device(view, "<phone & co>")
        check(phone && !phone.connected && phone.subtitle === "0 shared folders", "disconnected peer, no version: " + (phone && phone.subtitle))
        // The panel's argv gives the real core its lifecycle authority, and
        // the core's "managed" reaches the controls and the bar.
        check(service.canControlService && service.serviceActive && service.serviceAvailable,
              "service controls: " + JSON.stringify(service.lifecycle))
        check(panel.barIcon.codePointAt(0) === 0xf0160, "healthy bar glyph: " + panel.barIcon.codePointAt(0))
        console.log("FOAMY CHECKS " + checks)
        console.log("FOAMY DONE")
        Qt.quit()
      }, "the popup opens")
    })
  }
}
"""


def prepare(work, real_core=False):
    plugin = work / "plugin"
    shutil.copytree(SOURCE, plugin, ignore=shutil.ignore_patterns(".git", "__pycache__", "preview.png", "target"))
    if not real_core:
        # The fake core answers `stream`; `conflicts` goes to the real helper.
        core = plugin / "bin" / platform.machine() / "foamy-syncthing-helper"
        shutil.copyfile(core, work / "foamy-syncthing-helper")
        (work / "foamy-syncthing-helper").chmod(0o755)
        shutil.copyfile(SOURCE / "tests/fake-core.py", core)
        core.chmod(0o755)
    (plugin / "scripts/syncthing-install.sh").write_text(PROBE_STUB)
    return plugin


def write_snapshot(work, home):
    """The fake core's state. Docs lives in the shell's HOME and is configured
    as `~/Docs`: Syncthing keeps a folder path as entered, so the conflict scan
    has to expand it before the helper walks it. The scene later moves Photos
    to the empty Pictures."""
    docs = home / "Docs"
    (docs / "notes").mkdir(parents=True)
    (docs / "notes/plan.sync-conflict-20260101-000000-ABCDEFG.md").touch()
    photos = work / "Photos"
    (photos / ".stversions").mkdir(parents=True)
    (photos / ".stversions/a.sync-conflict-20260101-000000-ABCDEFG~20260102-000000.jpg").touch()
    (work / "Pictures").mkdir()
    snapshot = {
        "connection": {"online": True, "phase": "online", "fresh": True},
        "identity": {"deviceId": "LOCAL"},
        "lifecycle": {
            "active": True,
            "canControl": True,
            "targetMatch": True,
            "unitFileState": "enabled",
            "activeState": "active",
        },
        "webUi": {"url": "http://127.0.0.1:8384"},
        "counts": {"folders": 2, "devices": 2, "connectedDevices": 1},
        "devices": [
            {"id": "LOCAL", "name": "this", "connected": True},
            {"id": "PEER1", "name": "laptop", "connected": True},
            {"id": "PEER2", "name": "phone", "connected": False},
        ],
        "folders": [
            {
                "id": "docs",
                "label": "Documents",
                "path": "~/Docs",
                "devices": [{"id": "PEER1"}],
                "status": {"state": "idle", "globalFiles": 3, "globalBytes": 1000},
            },
            {
                "id": "photos",
                "label": "",
                "path": str(photos),
                "devices": [{"id": "PEER2"}],
                "status": {"state": "idle", "globalFiles": 9, "globalBytes": 90000},
            },
        ],
        "pendingFolders": {},
    }
    (work / "snapshot.json").write_text(json.dumps(snapshot))


class SilentBus:
    """A session bus (abstract socket). The first client is accepted and never
    answered, so pick-folder waits on it: its "connected <comm>" and, when
    its socket closes (the process is gone), "closed" go to `log`. Later
    clients get REJECTED for their AUTH line ("refused" in `log`), as a bus
    that does not take this user would answer."""

    def __init__(self, log):
        self.log = log
        self.pids = []
        self.clients = 0
        self.name = f"foamy-syncthing-picker-{os.getpid()}-{id(self)}"
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind("\0" + self.name)
        self.server.listen(4)
        threading.Thread(target=self.accept, daemon=True).start()

    @property
    def address(self):
        return f"unix:abstract={self.name}"

    def write(self, line):
        with open(self.log, "a") as output:
            output.write(line + "\n")

    def accept(self):
        while True:
            try:
                client, _ = self.server.accept()
            except OSError:
                return
            self.clients += 1
            threading.Thread(target=self.hold, args=(client, self.clients), daemon=True).start()

    def hold(self, client, number):
        pid = struct.unpack("3i", client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))[0]
        self.pids.append(pid)
        if number > 1:
            self.refuse(client, pid)
            return
        try:
            comm = Path(f"/proc/{pid}/comm").read_text().strip()
        except OSError:
            comm = "?"
        self.write(f"connected {comm} {pid}")
        while True:
            try:
                if not client.recv(4096):
                    break
            except OSError:
                break
        self.write(f"closed {pid}")
        client.close()

    def refuse(self, client, pid):
        received = b""
        try:
            while b"\r\n" not in received:
                data = client.recv(4096)
                if not data:
                    break
                received += data
            client.sendall(b"REJECTED EXTERNAL\r\n")
        except OSError:
            pass
        self.write(f"refused {pid}")
        client.close()

    def survivors(self):
        """Clients still alive (killed here, so none is left behind)."""
        left = [pid for pid in self.pids if Path(f"/proc/{pid}").exists()]
        for pid in left:
            try:
                os.kill(pid, 9)
            except OSError:
                pass
        return left

    def close(self):
        self.server.close()


def hung_probe_survivor(pid_file):
    """The hung check's child, killed here if it is still alive."""
    try:
        pid = int(pid_file.read_text())
    except (OSError, ValueError):
        return ["FOAMY FAIL the hung installation check never started its child"]
    if not Path(f"/proc/{pid}").exists():
        return []
    try:
        os.kill(pid, 9)
    except OSError:
        pass
    return [f"FOAMY FAIL the hung installation check's child outlived its deadline (pid {pid})"]


def run(
    host,
    shell,
    crash=False,
    blocked=False,
    snapshot=None,
    env=None,
    missing_helper=False,
    name="",
    probe_hang=False,
    probe_fail=False,
    theme="dark",
):
    """snapshot(work): the fake core's state (default: write_snapshot's); env:
    added to the shell's environment; missing_helper: no helper for this CPU;
    probe_hang: the first installation check hangs, and its child process
    must be gone when the run ends; probe_fail: the first installation check
    fails; theme: the harness theme, a name or a colors.toml path."""
    work = Path(tempfile.mkdtemp(prefix=f"foamy-syncthing-runtime-{host}-"))
    bus = None
    try:
        plugin = prepare(work)
        if missing_helper:
            (plugin / "bin" / platform.machine() / "foamy-syncthing-helper").unlink()
        qml = shell.replace("PROBE_LOG", json.dumps(str(work / "probe.log"))).replace(
            "CORE_LOG", json.dumps(str(work / "core.log"))
        )
        qml = qml.replace("PICKER_LOG", json.dumps(str(work / "picker.log")))
        qml = qml.replace("HANG_PID", json.dumps(str(work / "probe-hang.pid")))
        qml = qml.replace("SNAPSHOT", json.dumps(str(work / "snapshot.json")))
        (work / "probe.log").touch()
        (work / "core.log").touch()
        (work / "picker.log").touch()
        bus = SilentBus(work / "picker.log")
        runner = harness.Run(host, qml, plugin=plugin, prefix=f"foamy-syncthing-qs-{host}-", theme=theme)
        if snapshot is None:
            write_snapshot(work, Path(runner.env["HOME"]))
        else:
            (work / "snapshot.json").write_text(json.dumps(snapshot(work)))
        runner.env.update(
            {
                "FAKE_CORE_SNAPSHOT": str(work / "snapshot.json"),
                "FAKE_CORE_LOG": str(work / "core.log"),
                "FAKE_CORE_HELPER": str(work / "foamy-syncthing-helper"),
                "FOAMY_PROBE_LOG": str(work / "probe.log"),
                "FAKE_PICKER_BUS": bus.address,
            }
        )
        if crash or blocked or probe_hang:
            runner.env["FAKE_CORE_EXIT"] = "1"
        if probe_hang:
            runner.env["FAKE_PROBE_HANG"] = str(work / "probe-hang.pid")
        if probe_fail:
            runner.env["FAKE_PROBE_FAIL"] = str(work / "probe-failed")
        if blocked:
            runner.env["FAKE_PROBE_BLOCKING"] = str(Path(runner.env["HOME"]) / ".config/systemd/user/syncthing.service")
        runner.env.update(env or {})
        runner.run(timeout=90)
        lines = runner.log.splitlines()
        # Any QML warning from the plugin's own files fails the run too.
        warnings = [line for line in lines if "WARN" in line and "/plugin/" in line and "/plugin/kit/" not in line]
        checks = [line.split("FOAMY CHECKS")[1].strip() for line in lines if "FOAMY CHECKS" in line]
        label = f"{host}{' crash' if crash else ''}{' blocked' if blocked else ''}{' ' + name if name else ''}"
        left = bus.survivors()
        if left:
            warnings.append(f"FOAMY FAIL the folder chooser's helper outlived the run (pid {left})")
        if probe_hang:
            warnings += hung_probe_survivor(work / "probe-hang.pid")
        # The plugin runs no Omarchy command here; the harness stubs refuse and log any.
        warnings += [f"FOAMY FAIL host command ran: {line}" for line in runner.host_commands()]
        if runner.ok and not warnings:
            print(f"{label}: runtime passed ({checks[0] if checks else '?'} checks)")
            runner.cleanup()
            return True
        print(f"{label}: runtime FAILED; log at {runner.base}/qs.log")
        print("\n".join(warnings + runner.failures() or lines[-30:]))
        return False
    finally:
        if bus:
            bus.close()
        shutil.rmtree(work, ignore_errors=True)


def run_live(host):
    """LIVE_SHELL with the real core, a fake Syncthing in the shell's HOME and
    fake systemctl/busctl/syncthing first on PATH (no real service is ever
    asked)."""
    work = Path(tempfile.mkdtemp(prefix=f"foamy-syncthing-live-{host}-"))
    syncthing = None
    try:
        plugin = prepare(work, real_core=True)
        runner = harness.Run(
            host,
            LIVE_SHELL,
            plugin=plugin,
            prefix=f"foamy-syncthing-live-{host}-",
            fakebin={
                "systemctl": core_harness.SYSTEMCTL,
                "busctl": core_harness.BUSCTL,
                "syncthing": core_harness.SYNCTHING,
            },
        )
        home = Path(runner.env["HOME"])
        syncthing = fake_syncthing.FakeSyncthing(home)
        (home / "Docs").mkdir()
        syncthing.state["folders"] = [fake_syncthing.folder(home / "Docs")]
        config = Path(runner.env["XDG_STATE_HOME"]) / "syncthing"
        config.mkdir(parents=True)
        syncthing.state["paths"]["config"] = str(config / "config.xml")
        (config / "config.xml").write_text(
            f'<configuration><gui tls="false"><address>{syncthing.address}</address>'
            f"<apikey>{fake_syncthing.API_KEY}</apikey></gui></configuration>\n"
        )
        systemctl = work / "systemctl.json"
        systemctl.write_text(
            json.dumps(
                {
                    "LoadState": "loaded",
                    "ActiveState": "active",
                    "UnitFileState": "enabled",
                    "FragmentPath": "/usr/lib/systemd/user/syncthing.service",
                    "ExecStart": ["/usr/bin/syncthing", "serve", "--no-browser"],
                }
            )
        )
        for name in ("STCONFDIR", "STHOMEDIR"):
            runner.env.pop(name, None)
        runner.env.update(
            {
                "FAKE_CALLS": str(work / "calls.jsonl"),
                "FAKE_SYSTEMCTL_STATE": str(systemctl),
            }
        )
        runner.run(timeout=90)
        lines = runner.log.splitlines()
        warnings = [line for line in lines if "WARN" in line and "/plugin/" in line and "/plugin/kit/" not in line]
        warnings += [f"FOAMY FAIL host command ran: {line}" for line in runner.host_commands()]
        checks = [line.split("FOAMY CHECKS")[1].strip() for line in lines if "FOAMY CHECKS" in line]
        label = f"{host} live"
        if runner.ok and not warnings:
            print(f"{label}: runtime passed ({checks[0] if checks else '?'} checks)")
            runner.cleanup()
            return True
        print(f"{label}: runtime FAILED; log at {runner.base}/qs.log")
        print("\n".join(warnings + runner.failures() or lines[-30:]))
        return False
    finally:
        if syncthing:
            syncthing.close()
        shutil.rmtree(work, ignore_errors=True)


status = 0
# A dark theme with a dim red, for the error line's contrast check.
if not run("4.5", SHELL, theme=KIT / "tests/fixtures/themes/dim-red.toml"):
    status = 1
if not run("4.5", CRASH_SHELL, crash=True):
    status = 1
if not run("4.5", BLOCKED_SHELL, blocked=True):
    status = 1
if not run(
    "4.5",
    SERVICE_SHELL,
    snapshot=service_snapshot,
    env={"FAKE_CORE_FAIL": json.dumps(SERVICE_FAILURES)},
    name="service",
    probe_fail=True,
):
    status = 1
if not run("4.5", MISSING_SHELL, missing_helper=True, name="missing helper"):
    status = 1
if not run(
    "4.5",
    CHOOSER_SHELL,
    env={"FAKE_PICKER_PATH": "/home/me/" + "A folder with a long name/" * 4 + "Music"},
    name="chooser path",
):
    status = 1
if not run("4.5", SETTINGS_SHELL, name="settings"):
    status = 1
if not run("4.5", HUNG_PROBE_SHELL, probe_hang=True, name="hung probe"):
    status = 1
if not run("4.5", HUNG_PROBE_TEARDOWN_SHELL, probe_hang=True, name="hung probe teardown"):
    status = 1
if not run_live("4.5"):
    status = 1
sys.exit(status)
