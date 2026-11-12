"""Bound clipboard producers in real subprocesses and exercise the QML editor."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
KIT = Path(os.environ.get("FOAMY_KIT", Path.home() / "GitHub/Personal/foamy-kit"))


# The stand-ins (wl-paste, clipboard-fixture, a load-proof timeout) and the
# host isolation come from the kit checkout's preview/hostenv.py, which is not
# synced: without a checkout (GitHub's runner) the whole class skips.
try:
    sys.path.insert(0, str(KIT / "preview"))
    import hostenv
except ImportError:
    hostenv = None
finally:
    sys.path.pop(0)

HELPER = ROOT / "kit/clipboard.sh"


@unittest.skipIf(hostenv is None, f"foamy-kit's preview/hostenv.py not found at {KIT} (set FOAMY_KIT)")
class ClipboardTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.env = dict(os.environ, **hostenv.clipboard_fakes(self.base / "bin"))
        self.state = Path(self.env["TEST_CLIPBOARD_STATE"])

    def read(self, mode, selection="clipboard"):
        self.state.write_text(mode)
        started = time.monotonic()
        # Only the deadline modes end at clipboard.sh's 2 s; a paste that ends
        # by itself may take longer on a loaded machine (the stand-in timeout).
        deadline = mode in hostenv.CLIPBOARD_DEADLINE_MODES
        result = subprocess.run(
            ["bash", str(HELPER), selection], env=self.env, capture_output=True, timeout=10 if deadline else 90
        )
        if deadline:
            self.assertLess(time.monotonic() - started, 4)
        self.assertLessEqual(len(result.stdout), 100000)
        return result

    def test_unicode_empty_and_limit(self):
        for mode, expected in (("unicode", "/home/test/Blåbær 📁"), ("empty", ""), ("limit", "x" * 16384)):
            with self.subTest(mode=mode):
                result = self.read(mode)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout), expected)

    def test_primary_selection_and_single_line(self):
        result = self.read("lines", "primary")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), "one  two three")
        args = json.loads(Path(str(self.state) + ".args").read_text())
        self.assertEqual(args, ["--no-newline", "--type", "text", "--primary"])

    def test_oversized_nul_and_continuous_stream_are_rejected(self):
        for mode in ("large", "nul", "stream"):
            with self.subTest(mode=mode):
                result = self.read(mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, b"")
                self.assertEqual(result.stderr, b"")

    def test_total_deadline_covers_stalled_and_slow_sources(self):
        for mode in ("stall", "slow"):
            with self.subTest(mode=mode):
                result = self.read(mode)
                self.assertEqual(result.returncode, 137)

    def test_failure_and_invalid_selection(self):
        self.assertNotEqual(self.read("failure").returncode, 0)
        self.assertEqual(self.read("normal", "invalid").returncode, 2)

    def test_producer_ignoring_term_is_killed(self):
        result = self.read("ignore")
        self.assertEqual(result.returncode, 137)
        self.assert_producer_stopped()

    def assert_producer_stopped(self):
        pid = Path(str(self.state) + ".pid").read_text()
        status = Path(f"/proc/{pid}/stat")
        # The group leader can close stdout before a killed child is scheduled
        # to exit. Observe the child's state with a bounded wait, without the
        # exists/read race when /proc removes it between those two operations.
        # The kit's watchdog kills at 2 s; allow for a loaded machine.
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            try:
                state = status.read_text().split()[2]
            except (FileNotFoundError, ProcessLookupError):
                # Gone between open and read: /proc answers ESRCH then.
                return
            if state == "Z":
                return
            time.sleep(0.01)
        self.fail("clipboard producer survived its deadline")

    def test_watchdog_survives_wrapper_destruction(self):
        self.state.write_text("stall")
        with subprocess.Popen(
            ["bash", str(HELPER), "clipboard"], env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE
        ) as process:
            pid_file = Path(str(self.state) + ".pid")
            deadline = time.monotonic() + 2
            while not pid_file.exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertTrue(pid_file.exists(), "clipboard producer did not start")
            process.kill()
            process.communicate(timeout=4)
        self.assert_producer_stopped()

    def test_qml_editing_and_paste(self):
        runner = shutil.which("qs")
        if not runner or os.environ.get("FOAMY_TESTS_NO_QUICKSHELL"):
            # tests/suite.sh --ci sets FOAMY_TESTS_NO_QUICKSHELL: no host QML there.
            self.skipTest("Quickshell is unavailable")
        # Quickshell's statically registered Io plugin requires the qs executable.
        # A stock qmltestrunner cannot load it as a dynamic QML plugin.
        fixture = self.base / "shell.qml"
        fixture.write_text(
            """import QtQuick
import Quickshell
ShellRoot {
    FloatingWindow {
        title: "Foamy clipboard tests"
        visible: true
        implicitWidth: 500
        implicitHeight: 250
        Loader { anchors.fill: parent; source: %s }
    }
}
"""
            % json.dumps(str(ROOT / "tests/tst_clipboard.qml"))
        )
        # The kit's SafeTextField imports qs.Commons and qs.Ui from the host shell (read-only links).
        host = Path(os.environ.get("OMARCHY_SHELL_PATH", "/usr/share/omarchy/shell"))
        for name in ("Commons", "Ui"):
            (self.base / name).symlink_to(host / name, target_is_directory=True)
        runtime = self.base / "runtime"
        runtime.mkdir(mode=0o700)
        # No Omarchy bin dir, AppImage libraries, audio server, system or
        # session bus, or display: the paste needs none of them.
        env = hostenv.isolate(
            dict(
                self.env,
                QT_QPA_PLATFORM=os.environ.get("FOAMY_TEST_PLATFORM", "offscreen"),
                QT_QUICK_BACKEND="software",
                QT_QPA_PLATFORMTHEME="",
                XDG_RUNTIME_DIR=str(runtime),
            ),
            self.base,
        )
        if env["QT_QPA_PLATFORM"] == "wayland":
            # On the live compositor on purpose (FOAMY_TEST_PLATFORM=wayland):
            # its socket by full path, as XDG_RUNTIME_DIR is the test's own.
            env["WAYLAND_DISPLAY"] = str(Path(os.environ["XDG_RUNTIME_DIR"]) / os.environ["WAYLAND_DISPLAY"])
        # Its own process group: stop() also ends any clipboard.sh paste the
        # shell's Process left running (its trap takes the producer along).
        with tempfile.TemporaryFile("w+", errors="replace") as out:
            with hostenv.ProcessGroup(
                [runner, "-p", str(fixture), "--no-color"], env=env, stdout=out, stderr=subprocess.STDOUT
            ) as shell:
                status = shell.wait(180)
            # After stop(): nothing in the group writes to it any more.
            out.seek(0)
            output = out.read()
        self.assertIsNotNone(status, "qs did not finish within 180 s\n" + output)
        self.assertEqual(status, 0, output)
        self.assertRegex(output, r"CLIPBOARD_RESULT passed=[1-9][0-9]* failed=0")
        self.assertIn("failed=0", output, output)
        print(output[output.index("CLIPBOARD_RESULT") :].splitlines()[0])


if __name__ == "__main__":
    unittest.main()
