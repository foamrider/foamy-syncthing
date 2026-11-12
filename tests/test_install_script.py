"""scripts/syncthing-install.sh against fake commands in a temporary HOME:
the operation lock, its runtime directory and the blocking-file report.

Nothing here installs anything or opens a terminal: `omarchy`, `systemctl`
and `uwsm-app` are logging fakes, and PATH holds only them and the few real
tools the script needs (so no real Syncthing is found)."""

import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/syncthing-install.sh"
TOOLS = ("bash", "jq", "flock", "readlink", "timeout")
FAKE = """#!/usr/bin/bash
printf '%s\\n' "${0##*/} $*" >> "$TEST_LOG"
if [ "${0##*/}" = omarchy ]; then
  if [ -n "${TEST_DAEMON:-}" ]; then
    /usr/bin/sleep 30 </dev/null >/dev/null 2>&1 &
    echo $! > "$TEST_DAEMON"
  fi
  while [ -e "$TEST_GATE" ]; do /usr/bin/sleep 0.05; done
fi
"""


def wait_until(condition, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if condition():
            return True
        time.sleep(0.02)
    return False


class InstallScriptTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name)
        self.home = self.base / "home"
        self.home.mkdir()
        self.runtime = self.base / "runtime"
        self.runtime.mkdir(mode=0o700)
        self.bin = self.base / "bin"
        self.bin.mkdir()
        for tool in TOOLS:
            (self.bin / tool).symlink_to(Path("/usr/bin") / tool)
        for name in ("omarchy", "systemctl", "uwsm-app", "xdg-terminal-exec"):
            (self.bin / name).write_text(FAKE)
            (self.bin / name).chmod(0o755)
        self.log = self.base / "calls.log"
        self.log.touch()
        self.gate = self.base / "gate"
        self.env = {
            "HOME": str(self.home),
            "PATH": str(self.bin),
            "XDG_RUNTIME_DIR": str(self.runtime),
            "TEST_LOG": str(self.log),
            "TEST_GATE": str(self.gate),
            "LANG": "C.UTF-8",
        }
        self.processes = []

    def tearDown(self):
        for process in self.processes:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()

    def calls(self):
        return self.log.read_text().splitlines()

    def script(self, *args, env=None, stdin=""):
        return subprocess.run(
            ["/usr/bin/bash", str(SCRIPT), *args],
            env=env or self.env,
            input=stdin,
            capture_output=True,
            text=True,
            timeout=20,
            cwd=self.base,
        )

    def status(self, env=None):
        result = self.script("status", env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def start_run(self, **extra):
        self.gate.touch()
        process = subprocess.Popen(
            ["/usr/bin/bash", str(SCRIPT), "run", "install-package"],
            env=dict(self.env, **extra),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            start_new_session=True,
            cwd=self.base,
        )
        self.processes.append(process)
        self.assertTrue(wait_until(lambda: "omarchy pkg add syncthing" in self.calls()), self.calls())
        return process

    def finish(self, process):
        self.gate.unlink()
        self.assertTrue(wait_until(lambda: "systemctl --user enable --now syncthing.service" in self.calls()))
        output, error = process.communicate("\n", timeout=20)
        self.assertEqual(process.returncode, 0, error)
        self.assertIn("completed successfully", output)

    def test_without_a_runtime_directory_nothing_runs(self):
        for value in (None, "", "relative/dir", str(self.base / "missing")):
            env = dict(self.env)
            if value is None:
                del env["XDG_RUNTIME_DIR"]
            else:
                env["XDG_RUNTIME_DIR"] = value
            with self.subTest(XDG_RUNTIME_DIR=value):
                for args in (["status"], ["install"], ["run", "install-package"]):
                    result = self.script(*args, env=env, stdin="\n")
                    self.assertNotEqual(result.returncode, 0, args)
                    self.assertIn("XDG_RUNTIME_DIR is not set to a directory", result.stderr, args)
                self.assertEqual(self.calls(), [])

    def test_a_run_holds_the_lock_until_it_ends_and_leaves_no_marker(self):
        self.assertFalse(self.status()["operationRunning"])
        run = self.start_run()
        self.assertTrue(self.status()["operationRunning"])
        second = self.script("run", "install-package", stdin="\n")
        self.assertEqual(second.returncode, 1)
        self.assertIn("Another Syncthing operation is already running", second.stderr)
        self.assertEqual(self.calls().count("omarchy pkg add syncthing"), 1)
        self.finish(run)
        self.assertFalse(self.status()["operationRunning"])
        self.assertEqual(sorted(p.name for p in self.runtime.iterdir()), ["foamy-syncthing-install.lock"])
        # The next run takes the lock again.
        self.log.write_text("")
        self.finish(self.start_run())

    def test_a_killed_run_frees_the_lock(self):
        run = self.start_run()
        os.killpg(run.pid, signal.SIGKILL)
        run.wait()
        self.assertFalse(self.status()["operationRunning"])

    def test_a_process_the_install_leaves_behind_does_not_hold_the_lock(self):
        daemon_file = self.base / "daemon.pid"
        run = self.start_run(TEST_DAEMON=str(daemon_file))
        self.assertTrue(wait_until(daemon_file.exists))
        daemon = int(daemon_file.read_text())
        self.addCleanup(lambda: os.kill(daemon, signal.SIGKILL) if Path(f"/proc/{daemon}").exists() else None)
        self.finish(run)
        self.assertTrue(Path(f"/proc/{daemon}").exists(), "the stand-in daemon outlives the run")
        self.assertFalse(self.status()["operationRunning"])

    def test_status_names_the_file_that_blocks_the_install(self):
        self.assertEqual(self.status()["state"], "missing")
        self.assertEqual(self.status()["blocking"], "")
        unit = self.home / ".config/systemd/user/syncthing.service"
        unit.parent.mkdir(parents=True)
        unit.write_text("[Service]\n")
        status = self.status()
        self.assertEqual((status["state"], status["blocking"]), ("incomplete", str(unit)))
        link = self.home / ".local/bin/syncthing"
        link.parent.mkdir(parents=True)
        link.symlink_to(self.base / "gone")
        status = self.status()
        self.assertEqual((status["state"], status["blocking"]), ("incomplete", str(link)))
        custom = self.base / "config"
        status = self.status(dict(self.env, XDG_CONFIG_HOME=str(custom)))
        self.assertEqual(status["blocking"], str(link))
        link.unlink()
        self.assertEqual(self.status(dict(self.env, XDG_CONFIG_HOME=str(custom)))["state"], "missing")
        # The install refuses a blocked state and changes nothing.
        result = self.script("run", "install-package", stdin="\n")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("state: incomplete", result.stderr)
        self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
