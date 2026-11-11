"""Check conflict semantics and private stdin at the real process boundary.

FOAMY_HELPER: JSON argv prefix of the helper binary; the scan runs as
`<prefix> conflicts`. The default is this machine's bundled helper,
bin/<machine>/foamy-syncthing-helper."""

import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import time
import unittest

import hostenv

ROOT = Path(__file__).resolve().parents[1]
MAX_INPUT_BYTES = 1048576
GENERIC = b"Could not check for Syncthing conflicts.\n"


def scanner():
    if os.environ.get("FOAMY_HELPER"):
        prefix = json.loads(os.environ["FOAMY_HELPER"])
        return [str(Path(prefix[0]).resolve())] + prefix[1:] + ["conflicts"]
    return [str(ROOT / "bin" / platform.machine() / "foamy-syncthing-helper"), "conflicts"]


def scan(paths=None, raw=None, args=(), env=None, timeout=30):
    data = raw if raw is not None else json.dumps(paths, ensure_ascii=False).encode()
    return subprocess.run(scanner() + list(args), input=data, capture_output=True, timeout=timeout,
                          env=dict(os.environ, **(env or {})))


def flags(paths):
    result = scan(paths)
    if result.returncode != 0:
        raise AssertionError(result.stderr)
    return json.loads(result.stdout)


def has_conflicts(paths):
    return any(flags(paths))


class ConflictTests(unittest.TestCase):
    def test_only_matching_regular_files_count(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "nested").mkdir()
            (root / "directory.sync-conflict-test").mkdir()
            (root / "normal.txt").touch()
            os.mkfifo(root / "pipe.sync-conflict-test")
            self.assertFalse(has_conflicts([directory]))
            (root / "nested" / '.private 📁 "quote"\n.sync-conflict-test').touch()
            self.assertTrue(has_conflicts([directory]))

    def test_the_pattern_is_a_substring_match(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("sync-conflict-x", "a.sync-conflict", "a.SYNC-CONFLICT-x", "a.sync_conflict-x"):
                (root / name).touch()
            self.assertFalse(has_conflicts([directory]))
            (root / ".sync-conflict-").touch()
            self.assertTrue(has_conflicts([directory]))
            (root / ".sync-conflict-").unlink()
            name = os.fsencode(directory) + b"/\xff.sync-conflict-\xfe"
            with open(name, "wb"):
                pass
            self.assertTrue(has_conflicts([directory]))

    def test_versioned_copies_and_the_folder_marker_are_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # Syncthing's file versioning keeps resolved conflicts as
            # name.sync-conflict-…~timestamp in .stversions.
            for internal in (".stversions/nested", ".stfolder"):
                (root / internal).mkdir(parents=True)
                (root / internal / "a.sync-conflict-20260101-000000-ABCDEFG~20260102-000000.txt").touch()
            self.assertFalse(has_conflicts([directory]))
            # Only directories by those names are skipped, at any depth.
            (root / "docs" / ".stversions").mkdir(parents=True)
            (root / "docs" / ".stversions" / "b.sync-conflict-x").touch()
            self.assertFalse(has_conflicts([directory]))
            (root / "docs" / "a.sync-conflict-20260101-000000-ABCDEFG.txt").touch()
            self.assertTrue(has_conflicts([directory]))

    def test_flags_are_per_folder_and_in_order(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            for name in ("clean", "conflicted", "also-clean"):
                (base / name).mkdir()
            (base / "conflicted" / "b.sync-conflict-x").touch()
            paths = [str(base / name) for name in ("clean", "conflicted", "missing", "also-clean")]
            result = scan(paths)
            self.assertEqual((result.returncode, result.stdout, result.stderr),
                             (0, b"[false, true, false, false]\n", b""))
            self.assertEqual(scan([]).stdout, b"[]\n")
            self.assertEqual(flags([paths[1], paths[1]]), [True, True])

    def test_root_links_are_followed_but_descendant_links_are_not(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / "root"
            outside = base / "outside"
            root.mkdir()
            outside.mkdir()
            conflict = outside / "a.sync-conflict-test"
            conflict.touch()
            (root / "linked-directory").symlink_to(outside, target_is_directory=True)
            (root / "linked.sync-conflict-test").symlink_to(conflict)
            (root / "broken.sync-conflict-test").symlink_to(base / "missing")
            self.assertFalse(has_conflicts([str(root)]))
            configured_link = base / "configured-link"
            configured_link.symlink_to(outside, target_is_directory=True)
            self.assertTrue(has_conflicts([str(configured_link)]))
            (base / "loop").symlink_to(base / "loop")
            self.assertEqual(scan([str(base / "loop")]).returncode, 1)

    def test_empty_missing_and_non_directory_roots_remain_clear(self):
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "file.sync-conflict-test"
            file.touch()
            self.assertFalse(has_conflicts([]))
            self.assertFalse(has_conflicts([str(file), str(file / "missing"), directory + "/missing"]))
            second = Path(directory) / "second"
            second.mkdir()
            (second / "a.sync-conflict-test").touch()
            self.assertTrue(has_conflicts([directory + "/missing", str(second)]))
            # Relative roots resolve against the working directory.
            result = subprocess.run(scanner(), input=b'["second"]', capture_output=True, timeout=30, cwd=directory)
            self.assertEqual(result.stdout, b"[true]\n")

    def test_request_validation_and_unicode(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = [directory + '/private 📁 "quote"\n--exec', "-leading-dash"]
            self.assertEqual(flags(paths), [False, False])
            self.assertEqual(scan(raw=b'  ["a"]  \n').stdout, b"[false]\n")
            self.assertEqual(scan(raw=b" " * MAX_INPUT_BYTES + b"[]").returncode, 1)
            self.assertEqual(scan(raw=b" " * (MAX_INPUT_BYTES - 2) + b"[]").stdout, b"[]\n")
        invalid = [b"bad", b"\xff", b"{}", b"[7]", b'[""]', b'["nul\\u0000"]', b'["\\ud800"]', b"",
                   b'["a"] ["b"]', b"\xef\xbb\xbf[]", b'["\xff"]', b" " * (MAX_INPUT_BYTES + 1)]
        for raw in invalid:
            with self.subTest(raw=raw[:40]):
                result = scan(raw=raw)
                self.assertEqual((result.returncode, result.stdout, result.stderr), (1, b"", GENERIC))

    @unittest.skipIf(os.geteuid() == 0, "root bypasses directory permissions")
    def test_unreadable_directory_is_a_generic_scan_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            unreadable = Path(directory) / "private-Canary-847"
            unreadable.mkdir()
            unreadable.chmod(0)
            try:
                for paths in ([directory], [str(unreadable)]):
                    result = scan(paths)
                    self.assertEqual((result.returncode, result.stdout, result.stderr), (1, b"", GENERIC))
            finally:
                unreadable.chmod(0o700)

    def test_actual_worker_has_static_argv_and_no_children(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory) / 'private-Canary-847 📁 "quote"\n--exec'
            folder.mkdir()
            (folder / "a.sync-conflict-test").touch()
            expected = [os.fsencode(part) for part in scanner()]
            with subprocess.Popen(scanner(), stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
                # Hold stdin open so the production worker is observable on procfs.
                process.stdin.write(json.dumps([str(folder)], ensure_ascii=False).encode())
                process.stdin.flush()
                until = time.monotonic() + 5
                while True:
                    raw = Path(f"/proc/{process.pid}/cmdline").read_bytes()
                    argv = raw.split(b"\x00")[:-1]
                    # Without a binfmt emulator's prefix (qemu-aarch64-static <binary>).
                    if argv and os.path.basename(argv[0]).startswith(b"qemu-"):
                        argv = argv[2:]
                    if argv[:1] == expected[:1]:
                        break
                    self.assertLess(time.monotonic(), until)
                    time.sleep(0.01)
                self.assertEqual(argv, expected)
                self.assertNotIn(b"private-Canary-847", raw)
                tasks = Path(f"/proc/{process.pid}/task")
                children = "".join((task / "children").read_text() for task in tasks.iterdir())
                self.assertEqual(children.strip(), "")
                process.stdin.close()
                process.stdin = None
                output, error = process.communicate(timeout=3)
                self.assertEqual(process.returncode, 0, error)
                self.assertEqual(output, b"[true]\n")
                self.assertEqual(error, b"")
            result = scan([directory + "/missing"])
            self.assertEqual((result.returncode, result.stdout, result.stderr), (0, b"[false]\n", b""))

    def test_arguments_and_malformed_input_fail_without_disclosure(self):
        for args, request in [(["private-Canary-847"], b"[]"), ([], b'["private-Canary-847", 7]')]:
            result = scan(raw=request, args=args)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b"")
            self.assertNotIn(b"private-Canary-847", result.stderr)
        self.assertEqual(scan(raw=b"[]", args=["x"]).stderr, b"Conflict helper accepts stdin only.\n")
        self.assertEqual(scan(raw=b"[]", args=["x"]).returncode, 2)

    def test_stalled_stdin_is_bounded_by_the_helper_deadline(self):
        # The scanner gives up after 25 s on its own (the Python one through
        # SIGALRM); FOAMY_SYNCTHING_CONFLICTS_DEADLINE shortens that for tests.
        with subprocess.Popen(scanner(), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              env=dict(os.environ, FOAMY_SYNCTHING_CONFLICTS_DEADLINE="1")) as process:
            started = time.monotonic()
            process.wait(timeout=30)
            output, error = process.communicate()
            self.assertEqual(process.returncode, 1)
            self.assertEqual(output, b"")
            self.assertEqual(error, GENERIC)
            self.assertLess(time.monotonic() - started, 27)

    def test_qml_stdin_failure_timeout_and_destruction(self):
        runner = shutil.which("qs")
        if not runner:
            self.skipTest("Quickshell is unavailable")
        source = (ROOT / "Service.qml").read_text()
        methods = []
        for name in ("scanConflicts", "finishConflictScan"):
            start = source.index("  function " + name + "(")
            methods.append(source[start:source.index("\n  }", start) + 4])
        start = source.index("  // Folder ids whose directory holds")
        state_block = source[start:source.index("\n\n", start)]
        start = source.index("  property Process conflictScan:")
        process = source[start:source.index("  // One diagnostics target per shell", start)]
        start = source.index("  Component.onDestruction:")
        cleanup = source[start:source.index("    if (Plugin.ServiceRegistry", start)] + "  }"
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            # The helper as FoamyNative launches it: bin/$HOSTTYPE/<name>.
            helper_dir = base / "bin" / platform.machine()
            helper_dir.mkdir(parents=True)
            (base / "kit").mkdir()
            shutil.copy(ROOT / "kit/FoamyNative.js", base / "kit/FoamyNative.js")
            (base / "models").mkdir()
            shutil.copy(ROOT / "models/PanelModel.js", base / "models/PanelModel.js")
            stub = helper_dir / "foamy-syncthing-helper"
            state = base / "delivered.jsonl"
            stub.write_text('''#!/usr/bin/python3
import json, os, sys, time
from pathlib import Path
assert sys.argv[1:] == ["conflicts"], sys.argv
paths = json.load(sys.stdin)
with Path(os.environ["TEST_CONFLICT_STATE"]).open("a") as output:
    output.write(json.dumps({"paths":paths, "cmdline":Path("/proc/self/cmdline").read_bytes().decode()}) + "\\n")
if paths in (["stall"], ["destroy"]): time.sleep(30)
if paths == ["fail"]: sys.exit(1)
if paths == ["invalid"]: print("unexpected")
elif paths == ["short"]: print("[]")
else: print(json.dumps([path == "found" for path in paths]))
''')
            # Isolate the host adapter, while exercising the actual QML functions,
            # state, Process, deadline and destruction handler from Service.qml unchanged.
            stub.chmod(0o755)
            (base / "Scanner.qml").write_text('''import QtQuick
import Quickshell.Io
import "kit/FoamyNative.js" as FoamyNative
import "models/PanelModel.js" as PanelModel
QtObject {
  id: root
  property string pluginRoot: %s
  property string homePath: ""
  property bool online: true
  property var folders: []
  function tr(key) { return key }
%s
%s
%s
%s
}
''' % (json.dumps(str(base)), state_block, "\n".join(methods), process, cleanup))
            (base / "shell.qml").write_text('''import QtQuick
import QtTest
import Quickshell
import Quickshell.Io
import "kit/FoamyNative.js" as FoamyNative
ShellRoot {
  FloatingWindow {
    visible: true; implicitWidth: 300; implicitHeight: 100
    Item {
      id: root
      property bool ready: false
      property bool probeFinished: false
      property int probeExit: -1
      Timer { interval: 100; running: true; onTriggered: root.ready = true }
      Scanner { id: scanner }
      Component { id: factory; Scanner {} }
      Process { id: probe; onExited: function(code) { root.probeExit = code; root.probeFinished = true } }
      TestCase {
        name: "PrivateConflictScan"; when: root.ready
        function scan(paths) {
          scanner.folders = paths.map(function(path, index) { return {id:"f" + index, path:path} })
          scanner.scanConflicts(true)
          tryCompare(scanner, "conflictInput", "", 3000)
        }
        function test_1_stdin_results_per_folder() {
          scan(['private-Canary-847 📁 "quote"\\n--exec', '-leading-dash'])
          compare(scanner.conflictScanError, "")
          compare(scanner.hasConflicts, false)
          scan(["found"])
          compare(scanner.hasConflicts, true)
          scan(["clean", "found", "other"])
          compare(JSON.stringify(scanner.conflictFolderIds), JSON.stringify({f1: true}))
          // Syncthing's "~/" paths reach the helper expanded.
          scanner.homePath = "/home/me"
          scan(["~/Sync"])
          scanner.homePath = ""
        }
        function test_2_failure_retains_status_and_recovery() {
          scanner.conflictFolderIds = {f0: true}
          scan(["fail"])
          verify(scanner.conflictScanError !== "")
          compare(scanner.hasConflicts, true)
          scan(["invalid"])
          verify(scanner.conflictScanError !== "")
          scan(["short"])
          verify(scanner.conflictScanError !== "")
          compare(scanner.hasConflicts, true)
          scan(["clear"])
          compare(scanner.conflictScanError, "")
          compare(scanner.hasConflicts, false)
        }
        function test_3_deadline_and_busy() {
          scanner.conflictDeadline.interval = 250
          scanner.conflictFolderIds = {f0: true}
          scanner.folders = [{id:"f0", path:"stall"}]
          scanner.scanConflicts()
          scanner.scanConflicts()
          tryCompare(scanner, "conflictInput", "", 3000)
          verify(scanner.conflictScanError !== "")
          compare(scanner.hasConflicts, true)
          wait(100)
          compare(scanner.conflictScan.running, false)
          scanner.conflictDeadline.interval = 30000
          scan(["clear"])
          compare(scanner.conflictScanError, "")
        }
        function test_4_limits_empty_and_offline() {
          scanner.conflictFolderIds = {f0: true}
          scan(["x".repeat(262145)])
          verify(scanner.conflictScanError !== "")
          compare(scanner.hasConflicts, true)
          scan([])
          compare(scanner.conflictScanError, "")
          compare(scanner.hasConflicts, false)
          scanner.online = false
          scan(["found"])
          compare(scanner.hasConflicts, false)
          scanner.online = true
        }
        function test_5_destruction_stops_worker() {
          var worker = factory.createObject(root)
          worker.folders = [{id:"f0", path:"destroy"}]
          worker.scanConflicts()
          tryVerify(function() { return Number(worker.conflictScan.processId) > 0 }, 1000)
          var pid = worker.conflictScan.processId
          worker.destroy()
          wait(100)
          probe.command = ["/usr/bin/test", "!", "-e", "/proc/" + pid]
          root.probeFinished = false; probe.running = true
          tryCompare(root, "probeFinished", true, 1000)
          compare(root.probeExit, 0)
        }
        function test_6_snapshots_with_the_same_folders_skip_the_walk() {
          scanner.folders = [{id:"a", path:"counted"}]
          scanner.scanConflicts()
          tryCompare(scanner, "conflictInput", "", 3000)
          // A new snapshot rebuilds the array with the same folders: no walk.
          scanner.folders = [{id:"a", path:"counted"}]
          scanner.scanConflicts()
          compare(scanner.conflictInput, "")
          compare(scanner.conflictScan.running, false)
          // A changed folder set and the periodic timer (force) do walk.
          scanner.folders = [{id:"a", path:"counted"}, {id:"b", path:"counted"}]
          scanner.scanConflicts()
          tryCompare(scanner, "conflictInput", "", 3000)
          scanner.scanConflicts(true)
          tryCompare(scanner, "conflictInput", "", 3000)
        }
        function test_7_a_missing_helper_is_reported_once() {
          probe.command = ["/usr/bin/mv", STUB, STUB + ".off"]
          root.probeFinished = false; probe.running = true
          tryCompare(root, "probeFinished", true, 1000)
          scanner.conflictFolderIds = {f0: true}
          scan(["found"])
          compare(scanner.conflictScanError, FoamyNative.UNAVAILABLE_TEXT)
          compare(scanner.conflictScanUnavailable, true)
          compare(scanner.hasConflicts, true)
          scanner.folders = [{id:"z", path:"other"}]
          scanner.scanConflicts(true)
          compare(scanner.conflictInput, "")
          compare(scanner.conflictScan.running, false)
        }
        onCompletedChanged: if (completed) {
          console.log("CONFLICT_SCAN passed=" + qtest_results.passCount + " failed=" + qtest_results.failCount)
          Qt.quit()
        }
      }
    }
  }
}
''')
            shell = base / "shell.qml"
            shell.write_text(shell.read_text().replace("STUB", json.dumps(str(stub))))
            runtime = base / "runtime"
            runtime.mkdir(mode=0o700)
            env = hostenv.shell_env(QT_QPA_PLATFORM="offscreen", QT_QUICK_BACKEND="software",
                                    QT_QPA_PLATFORMTHEME="", XDG_RUNTIME_DIR=str(runtime), TEST_CONFLICT_STATE=str(state))
            result = subprocess.run([runner, "-p", str(base), "--no-color"], env=env,
                                    text=True, capture_output=True, timeout=15)
            output = result.stdout + result.stderr
            self.assertEqual(result.returncode, 0, output)
            self.assertIn("CONFLICT_SCAN passed=8 failed=0", output)
            records = [json.loads(line) for line in state.read_text().splitlines()]
            self.assertEqual(records[0]["paths"], ['private-Canary-847 📁 "quote"\n--exec', '-leading-dash'])
            self.assertIn(["/home/me/Sync"], [record["paths"] for record in records])
            for record in records:
                # The paths travel on stdin only.
                self.assertEqual(record["cmdline"].split("\x00")[:-1], ["/usr/bin/python3", str(stub), "conflicts"])
            self.assertEqual(sum(record["paths"] == ["stall"] for record in records), 1)
            self.assertEqual(sum(record["paths"] == ["counted"] for record in records), 1)
            self.assertEqual(sum(record["paths"] == ["counted", "counted"] for record in records), 2)
            print("CONFLICT_SCAN passed=8 failed=0")


if __name__ == "__main__":
    unittest.main()
