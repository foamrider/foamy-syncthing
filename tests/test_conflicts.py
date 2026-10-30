"""Check conflict semantics and private stdin at the real process boundary."""

import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "scripts/syncthing-conflicts.py"
spec = importlib.util.spec_from_file_location("syncthing_conflicts", HELPER)
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


class ConflictTests(unittest.TestCase):
    def test_only_matching_regular_files_count(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "nested").mkdir()
            (root / "directory.sync-conflict-test").mkdir()
            (root / "normal.txt").touch()
            os.mkfifo(root / "pipe.sync-conflict-test")
            self.assertFalse(helper.has_conflicts([directory]))
            (root / "nested" / '.private 📁 "quote"\n.sync-conflict-test').touch()
            self.assertTrue(helper.has_conflicts([directory]))

    def test_versioned_copies_and_the_folder_marker_are_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # Syncthing's file versioning keeps resolved conflicts as
            # name.sync-conflict-…~timestamp in .stversions.
            for internal in (".stversions/nested", ".stfolder"):
                (root / internal).mkdir(parents=True)
                (root / internal / "a.sync-conflict-20260101-000000-ABCDEFG~20260102-000000.txt").touch()
            self.assertFalse(helper.has_conflicts([directory]))
            (root / "docs").mkdir()
            (root / "docs" / "a.sync-conflict-20260101-000000-ABCDEFG.txt").touch()
            self.assertTrue(helper.has_conflicts([directory]))

    def test_flags_are_per_folder_and_in_order(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            for name in ("clean", "conflicted", "also-clean"):
                (base / name).mkdir()
            (base / "conflicted" / "b.sync-conflict-x").touch()
            paths = [str(base / name) for name in ("clean", "conflicted", "missing", "also-clean")]
            self.assertEqual(helper.conflict_flags(paths), [False, True, False, False])
            result = subprocess.run(["/usr/bin/python3", str(HELPER)], input=json.dumps(paths).encode(),
                                    capture_output=True, timeout=3)
            self.assertEqual((result.returncode, result.stdout), (0, b"[false, true, false, false]\n"))

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
            self.assertFalse(helper.has_conflicts([str(root)]))
            configured_link = base / "configured-link"
            configured_link.symlink_to(outside, target_is_directory=True)
            self.assertTrue(helper.has_conflicts([str(configured_link)]))

    def test_empty_missing_and_non_directory_roots_remain_clear(self):
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "file.sync-conflict-test"
            file.touch()
            self.assertFalse(helper.has_conflicts([]))
            self.assertFalse(helper.has_conflicts([str(file), str(file / "missing"), directory + "/missing"]))
            second = Path(directory) / "second"
            second.mkdir()
            (second / "a.sync-conflict-test").touch()
            self.assertTrue(helper.has_conflicts([directory + "/missing", str(second)]))

    def test_request_validation_and_unicode(self):
        paths = ['/private 📁 "quote"\n--exec', "-leading-dash"]
        self.assertEqual(helper.read_paths(io.BytesIO(json.dumps(paths).encode())), paths)
        invalid = [b"bad", b"\xff", b"{}", b'[7]', b'[""]', b'["nul\\u0000"]',
                   b'["\\ud800"]', b" " * (helper.MAX_INPUT_BYTES + 1)]
        for raw in invalid:
            with self.subTest(raw=raw[:40]), self.assertRaises((ValueError, UnicodeError)):
                helper.read_paths(io.BytesIO(raw))

    def test_detection_stops_without_buffering_a_wide_directory(self):
        class Entry:
            name = "first.sync-conflict-test"

            def is_dir(self, follow_symlinks):
                return False

            def is_file(self, follow_symlinks):
                return True

        class Entries:
            closed = False
            visited = False

            def __next__(self):
                if self.visited:
                    raise AssertionError("scan continued after finding a conflict")
                self.visited = True
                return Entry()

            def close(self):
                self.closed = True

        entries = Entries()
        with tempfile.TemporaryDirectory() as directory, patch.object(helper.os, "scandir", return_value=entries):
            self.assertTrue(helper.has_conflicts([directory]))
        self.assertTrue(entries.closed)

    @unittest.skipIf(os.geteuid() == 0, "root bypasses directory permissions")
    def test_unreadable_directory_is_a_generic_scan_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            unreadable = Path(directory) / "private-Canary-847"
            unreadable.mkdir()
            unreadable.chmod(0)
            try:
                result = subprocess.run(["/usr/bin/python3", str(HELPER)],
                                        input=json.dumps([directory]).encode(), capture_output=True, timeout=3)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stdout, b"")
                self.assertEqual(result.stderr, b"Could not check for Syncthing conflicts.\n")
            finally:
                unreadable.chmod(0o700)

    def test_scan_failures_never_report_false_or_echo_paths(self):
        private = "/private-Canary-847"
        with patch.object(sys, "argv", [str(HELPER)]), \
                patch.object(sys, "stdin", type("Input", (), {"buffer": io.BytesIO(b'["/private-Canary-847"]')})()), \
                patch.object(sys, "stdout", io.StringIO()) as output, \
                patch.object(sys, "stderr", io.StringIO()) as error, \
                patch.object(helper, "conflict_flags", side_effect=PermissionError(private)):
            self.assertEqual(helper.main(), 1)
            self.assertEqual(output.getvalue(), "")
            self.assertEqual(error.getvalue(), "Could not check for Syncthing conflicts.\n")

    def test_actual_worker_has_static_argv_and_no_children(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory) / 'private-Canary-847 📁 "quote"\n--exec'
            folder.mkdir()
            (folder / "a.sync-conflict-test").touch()
            with subprocess.Popen(["/usr/bin/python3", str(HELPER)], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
                # Hold stdin open so the production worker is observable on procfs.
                process.stdin.write(json.dumps([str(folder)], ensure_ascii=False).encode())
                process.stdin.flush()
                until = time.monotonic() + 2
                while True:
                    argv = Path(f"/proc/{process.pid}/cmdline").read_bytes()
                    if argv.startswith(b"/usr/bin/python3\x00"):
                        break
                    self.assertLess(time.monotonic(), until)
                    time.sleep(0.01)
                self.assertEqual(argv.split(b"\x00")[:-1], [b"/usr/bin/python3", os.fsencode(HELPER)])
                self.assertNotIn(b"private-Canary-847", argv)
                self.assertEqual(Path(f"/proc/{process.pid}/task/{process.pid}/children").read_text().strip(), "")
                process.stdin.close()
                process.stdin = None
                output, error = process.communicate(timeout=3)
                self.assertEqual(process.returncode, 0, error)
                self.assertEqual(output, b"[true]\n")
                self.assertEqual(error, b"")
            result = subprocess.run(["/usr/bin/python3", str(HELPER)],
                                    input=json.dumps([directory + "/missing"]).encode(),
                                    capture_output=True, timeout=3)
            self.assertEqual((result.returncode, result.stdout, result.stderr), (0, b"[false]\n", b""))

    def test_arguments_and_malformed_input_fail_without_disclosure(self):
        for args, request in [(["private-Canary-847"], b"[]"), ([], b'["private-Canary-847", 7]')]:
            result = subprocess.run(["/usr/bin/python3", str(HELPER), *args], input=request,
                                    capture_output=True, timeout=3)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b"")
            self.assertNotIn(b"private-Canary-847", result.stderr)

    def test_stalled_stdin_is_bounded_by_the_helper_deadline(self):
        # Shorten the production alarm only in this fixture, without a test CLI API.
        source = ("import importlib.util, signal; "
                  f"s=importlib.util.spec_from_file_location('helper',{str(HELPER)!r}); "
                  "m=importlib.util.module_from_spec(s); s.loader.exec_module(m); "
                  "alarm=signal.alarm; m.signal.alarm=lambda n: alarm(1 if n else 0); "
                  "raise SystemExit(m.main())")
        with subprocess.Popen(["/usr/bin/python3", "-c", source], stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
            process.wait(timeout=3)
            output, error = process.communicate()
            self.assertEqual(process.returncode, 1)
            self.assertEqual(output, b"")
            self.assertEqual(error, b"Could not check for Syncthing conflicts.\n")

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
            helper_dir = base / "scripts"
            helper_dir.mkdir(parents=True)
            state = base / "delivered.jsonl"
            (helper_dir / HELPER.name).write_text('''import json, os, sys, time
from pathlib import Path
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
            (base / "Scanner.qml").write_text('''import QtQuick
import Quickshell.Io
QtObject {
  id: root
  property string pluginRoot: %s
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
        onCompletedChanged: if (completed) {
          console.log("CONFLICT_SCAN passed=" + qtest_results.passCount + " failed=" + qtest_results.failCount)
          Qt.quit()
        }
      }
    }
  }
}
''')
            runtime = base / "runtime"
            runtime.mkdir(mode=0o700)
            env = dict(os.environ, QT_QPA_PLATFORM="offscreen", QT_QUICK_BACKEND="software",
                       QT_QPA_PLATFORMTHEME="", XDG_RUNTIME_DIR=str(runtime), TEST_CONFLICT_STATE=str(state))
            result = subprocess.run([runner, "-p", str(base), "--no-color"], env=env,
                                    text=True, capture_output=True, timeout=15)
            output = result.stdout + result.stderr
            self.assertEqual(result.returncode, 0, output)
            self.assertIn("CONFLICT_SCAN passed=7 failed=0", output)
            records = [json.loads(line) for line in state.read_text().splitlines()]
            self.assertEqual(records[0]["paths"], ['private-Canary-847 📁 "quote"\n--exec', '-leading-dash'])
            for record in records:
                self.assertEqual(record["cmdline"].split("\x00")[:-1], ["/usr/bin/python3", str(helper_dir / HELPER.name)])
            self.assertEqual(sum(record["paths"] == ["stall"] for record in records), 1)
            self.assertEqual(sum(record["paths"] == ["counted"] for record in records), 1)
            self.assertEqual(sum(record["paths"] == ["counted", "counted"] for record in records), 2)
            print("CONFLICT_SCAN passed=7 failed=0")


if __name__ == "__main__":
    unittest.main()
