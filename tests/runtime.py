#!/usr/bin/env python3
"""Load the real Service.qml once and Panel.qml three times (one bar per
monitor) in an isolated offscreen Quickshell against a host shell, through
foamy-kit's harness, with tests/fake-core.py in place of the native core and
a counting stub for the install probe.

  python3 tests/runtime.py

The host is Omarchy 4.5, the installed /usr/share/omarchy/shell
(FOAMY_HOST_45 picks another 4.5 tree).

Checks: one service and one settings leader for three bars; the core starts
without --desktop-authorized; the install probe stops once Syncthing is
online; snapshots with the same folders start no conflict walk; .stversions
is ignored and conflicts are flagged per folder, a "~/" folder path included;
the popup body is built on open only; row delegates survive a snapshot (keyed
model); settings open; a
folder chooser left open goes with its Panel (the real pick-folder helper,
blocked on a session bus that never answers, is gone once the Panel is
destroyed and nothing of it outlives the run); a crash-looping core keeps
being retried and right-click/open starts it now.
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

SOURCE = Path(__file__).resolve().parents[1]

PROBE_STUB = """#!/bin/bash
printf '%s\\n' "$1" >> "$FOAMY_PROBE_LOG"
printf '{"state":"existing","label":"Existing installation found: working","executable":"/usr/bin/syncthing","operationRunning":false}\\n'
"""

SHELL = r'''
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
    step.start()
  }

  Timer {
    id: step
    interval: 2500
    property int n: 0
    property int probesOnline: 0
    onTriggered: {
      if (n === 0) {
        check(Plugin.ServiceRegistry.instance === service, "service registered")
        check(Kit.FoamyShared.memberCount("syncthing") === 3, "three members")
        check(service.preferences.minimumConnectedDevices === 2 && service.settingsReady, "leader fed settings")
        check(service.online, "online through the fake core")
        check(coreLog.text().indexOf("--desktop-authorized") < 0 && coreLog.text().indexOf("--probe-interval-seconds") >= 0, "no desktop bridge flag: " + coreLog.text().slice(0, 200))
        check(shell.probes() >= 1, "install probe ran while offline")
        check(JSON.stringify(service.conflictFolderIds) === JSON.stringify({ docs: true }), "per-folder conflicts, .stversions ignored: " + JSON.stringify(service.conflictFolderIds))
        check(service.folderRows.length === 2 && service.folderRows[0].label === "Documents", "configured label first")
        // Escaped icon-font glyphs (\u{…}) still decode: conflicts → the "notify" glyph.
        check(panels[0].barIcon.codePointAt(0) === 0xf09e0 && panels[0].barIcon.length === 2, "bar glyph decodes: " + panels[0].barIcon.codePointAt(0))
        check(service.tooFewDevices === false && service.deviceCounts.connected === 2, "device counts include this computer")
        step.probesOnline = shell.probes()
        shell.scans = 0
        // Snapshots with the same folders (refreshes) must not walk again.
        service.refresh(); service.refresh()
        check(panels[0].popup.bodyItem === null && panels[1].popup.bodyItem === null, "bodies lazy before open")
        panels[1].open()
      } else if (n === 1) {
        var view = panels[1].popup.bodyItem
        check(view !== null && view.service === service, "body built on open")
        check(panels[0].popup.bodyItem === null, "other monitors stay lazy")
        check(view && view.rowCount === 2, "two folder rows")
        savedRow = view ? view.rowItem(0) : null
        check(savedRow !== null, "row delegate exists")
        check(view && view.listWheel && view.listWheel.target !== null, "the overview list scrolls with the kit wheel handling")
        service.refresh()
      } else if (n === 2) {
        check(panels[1].popup.bodyItem.rowItem(0) === savedRow, "row delegate survives a snapshot (keyed model)")
        // FoamyPrefsQueue: an inline save shows at once in the view, before the host reflects it.
        panels[1].save("refreshIntervalSec", 300)
        check(panels[1].popup.bodyItem.settings.refreshIntervalSec === 300, "saved value shows before it is reflected")
        check(panels[1].bar.saved.length === 1 && panels[1].bar.saved[0].settings.refreshIntervalSec === 300, "saved inline through the bar")
        check(shell.scans === 0, "no conflict walk for unchanged folders (" + shell.scans + ")")
        check(shell.probes() === step.probesOnline, "install probe idle while online (" + step.probesOnline + " -> " + shell.probes() + ")")
        panels[1].openSettings()
      } else if (n === 3) {
        check(panels[1].editingSettings, "settings open")
        panels[1].close()
      } else if (n === 4) {
        check(!panels[1].opened && !panels[1].editingSettings, "closing leaves settings")
        // The settings IPC opens a closed popup straight into Settings.
        panels[2].openSettings()
      } else if (n === 5) {
        check(panels[2].opened && panels[2].editingSettings, "openSettings() on a closed popup shows Settings")
        panels[2].close()
        // The folder chooser: the helper connects to a bus that never answers.
        panels[0].open()
        panels[0].browseFolder()
      } else if (n === 6) {
        check(panels[0].returningFromBrowser, "the chooser runs")
        check(pickerLog.text().indexOf("connected foamy-syncthing") >= 0, "pick-folder reached the session bus: " + pickerLog.text())
        panels[0].destroy()
      } else if (n === 7) {
        check(pickerLog.text().indexOf("closed") >= 0, "the chooser's helper went with its Panel: " + pickerLog.text())
        check(Kit.FoamyShared.memberCount("syncthing") === 2, "leader left, re-elected")
        console.log("FOAMY CHECKS " + checks)
        console.log("FOAMY DONE")
        Qt.quit()
        return
      }
      n++
      restart()
    }
  }
}
'''

CRASH_SHELL = r'''
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
    step.start()
  }
  Timer {
    id: step
    interval: 4000
    property int n: 0
    property int before: 0
    onTriggered: {
      if (n === 0) {
        var core = service.core
        check(core.restartAttempts === 4, "three fast restarts, then a slow one (" + core.restartAttempts + ")")
        check(core.restartTimer.running && core.restartTimer.interval === 30000, "slow retry scheduled, not given up")
        check(core.desiredRunning, "still wants to run")
        before = shell.starts()
        service.recover()
      } else {
        check(shell.starts() > before, "recover() starts the core right away")
        console.log("FOAMY CHECKS " + checks)
        console.log("FOAMY DONE")
        Qt.quit()
        return
      }
      n++
      restart()
    }
  }
}
'''


def prepare(work):
    plugin = work / "plugin"
    shutil.copytree(SOURCE, plugin, ignore=shutil.ignore_patterns(".git", "__pycache__", "preview.png", "target"))
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
    has to expand it before the helper walks it."""
    docs = home / "Docs"
    (docs / "notes").mkdir(parents=True)
    (docs / "notes/plan.sync-conflict-20260101-000000-ABCDEFG.md").touch()
    photos = work / "Photos"
    (photos / ".stversions").mkdir(parents=True)
    (photos / ".stversions/a.sync-conflict-20260101-000000-ABCDEFG~20260102-000000.jpg").touch()
    snapshot = {
        "connection": {"online": True, "phase": "online", "fresh": True},
        "identity": {"deviceId": "LOCAL"},
        "lifecycle": {"active": True, "canControl": True, "targetMatch": True, "unitFileState": "enabled", "activeState": "active"},
        "webUi": {"url": "http://127.0.0.1:8384"},
        "counts": {"folders": 2, "devices": 2, "connectedDevices": 1},
        "devices": [{"id": "LOCAL", "name": "this", "connected": True}, {"id": "PEER1", "name": "laptop", "connected": True},
                    {"id": "PEER2", "name": "phone", "connected": False}],
        "folders": [
            {"id": "docs", "label": "Documents", "path": "~/Docs", "devices": [{"id": "PEER1"}],
             "status": {"state": "idle", "globalFiles": 3, "globalBytes": 1000}},
            {"id": "photos", "label": "", "path": str(photos), "devices": [{"id": "PEER2"}],
             "status": {"state": "idle", "globalFiles": 9, "globalBytes": 90000}},
        ],
        "pendingFolders": {},
    }
    (work / "snapshot.json").write_text(json.dumps(snapshot))


class SilentBus:
    """A session bus (abstract socket) that accepts and never answers, so
    pick-folder waits on it. Each client's "connected <comm>" and, when its
    socket closes (the process is gone), "closed" go to `log`."""

    def __init__(self, log):
        self.log = log
        self.pids = []
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
            threading.Thread(target=self.hold, args=(client,), daemon=True).start()

    def hold(self, client):
        pid = struct.unpack("3i", client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))[0]
        self.pids.append(pid)
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


def run(host, shell, crash=False):
    work = Path(tempfile.mkdtemp(prefix=f"foamy-syncthing-runtime-{host}-"))
    bus = None
    try:
        plugin = prepare(work)
        qml = shell.replace("PROBE_LOG", json.dumps(str(work / "probe.log"))).replace("CORE_LOG", json.dumps(str(work / "core.log")))
        qml = qml.replace("PICKER_LOG", json.dumps(str(work / "picker.log")))
        (work / "probe.log").touch()
        (work / "core.log").touch()
        (work / "picker.log").touch()
        bus = SilentBus(work / "picker.log")
        runner = harness.Run(host, qml, plugin=plugin, prefix=f"foamy-syncthing-qs-{host}-")
        write_snapshot(work, Path(runner.env["HOME"]))
        runner.env.update({"FAKE_CORE_SNAPSHOT": str(work / "snapshot.json"), "FAKE_CORE_LOG": str(work / "core.log"),
                           "FAKE_CORE_HELPER": str(work / "foamy-syncthing-helper"),
                           "FOAMY_PROBE_LOG": str(work / "probe.log"), "FAKE_PICKER_BUS": bus.address})
        if crash:
            runner.env["FAKE_CORE_EXIT"] = "1"
        runner.run(timeout=90)
        lines = runner.log.splitlines()
        # Any QML warning from the plugin's own files fails the run too.
        warnings = [line for line in lines if "WARN" in line and "/plugin/" in line and "/plugin/kit/" not in line]
        checks = [line.split("FOAMY CHECKS")[1].strip() for line in lines if "FOAMY CHECKS" in line]
        label = f"{host}{' crash' if crash else ''}"
        left = bus.survivors()
        if left:
            warnings.append(f"FOAMY FAIL the folder chooser's helper outlived the run (pid {left})")
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


status = 0
if not run("4.5", SHELL):
    status = 1
if not run("4.5", CRASH_SHELL, crash=True):
    status = 1
sys.exit(status)
