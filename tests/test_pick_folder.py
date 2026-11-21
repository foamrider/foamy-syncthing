"""`pick-folder` through the process: the JSON line and exit code the panel
reads (PanelModel.folderPickReply), on a bus the test owns or on none.

The helper never sees the caller's session bus: every call builds its
environment from scratch, sets or leaves out DBUS_SESSION_BUS_ADDRESS itself
and runs with the test's own XDG_RUNTIME_DIR (without it, the helper would
dial /run/user/<uid>/bus and open a dialog on the user's screen). The
private dbus-daemon has no service directory, so nothing can be started on
it, and no portal answers.

FOAMY_HELPER: JSON argv prefix of the helper (default: bin/<machine>/…)."""

import json
import os
import platform
import select
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DBUS_DAEMON = shutil.which("dbus-daemon")
# A unix socket path must fit in sun_path (108 bytes with its terminator).
SOCKET_PATH_MAX = 100


def helper():
    if os.environ.get("FOAMY_HELPER"):
        prefix = json.loads(os.environ["FOAMY_HELPER"])
        return [str(Path(prefix[0]).resolve())] + prefix[1:] + ["pick-folder"]
    return [str(ROOT / "bin" / platform.machine() / "foamy-syncthing-helper"), "pick-folder"]


class PickFolderTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="foamy-syncthing-pick-")
        if len(os.fsencode(Path(temp.name) / "runtime" / "bus")) > SOCKET_PATH_MAX:
            # A long TMPDIR: the daemon could not bind its socket there.
            temp.cleanup()
            temp = tempfile.TemporaryDirectory(prefix="foamy-syncthing-pick-", dir="/tmp")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.runtime = self.root / "runtime"
        self.runtime.mkdir(mode=0o700)

    def pick(self, *args, bus):
        """Runs pick-folder with `bus` as the session bus address (None: not
        set); returns (exit code, reply)."""
        env = {"PATH": "/usr/bin:/bin", "HOME": str(self.root), "XDG_RUNTIME_DIR": str(self.runtime), "LANG": "C.UTF-8"}
        if bus is not None:
            env["DBUS_SESSION_BUS_ADDRESS"] = bus
        self.assertTrue(
            os.path.isabs(env["XDG_RUNTIME_DIR"]) and env["XDG_RUNTIME_DIR"].startswith(str(self.root)),
            f"pick-folder would look for the bus in {env['XDG_RUNTIME_DIR']}",
        )
        result = subprocess.run(helper() + list(args), env=env, capture_output=True, timeout=30)
        lines = result.stdout.splitlines()
        self.assertEqual(len(lines), 1, result)
        return result.returncode, json.loads(lines[0])

    def test_more_than_two_arguments_are_refused(self):
        code, reply = self.pick("a", "b", "c", bus=f"unix:path={self.root}/absent")
        self.assertEqual((code, reply["ok"], reply["code"]), (1, False, "INVALID_ARGUMENT"))

    def test_without_a_session_bus(self):
        # A dead socket, an address without a unix socket, and none at all
        # (then $XDG_RUNTIME_DIR/bus, which the test's directory does not have).
        for bus in (f"unix:path={self.root}/absent", "tcp:host=localhost,port=1", None):
            with self.subTest(bus=bus):
                code, reply = self.pick(bus=bus)
                self.assertEqual((code, reply["ok"], reply["code"]), (1, False, "NO_SESSION_BUS"))

    def test_a_real_bus_without_a_portal_is_unavailable(self):
        # A real daemon accepts the client's authentication, Hello, AddMatch
        # and OpenFile; with no portal on the bus the reply is "unavailable".
        # A host without dbus-daemon fails here instead of skipping the case.
        self.assertIsNotNone(DBUS_DAEMON, "dbus-daemon is not on PATH: install the dbus package")
        socket = self.runtime / "bus"
        config = self.root / "bus.conf"
        config.write_text(f"""<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path={socket}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
""")
        daemon = subprocess.Popen(
            [DBUS_DAEMON, f"--config-file={config}", "--nofork", "--nopidfile", "--print-address=1"],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            env={"PATH": "/usr/bin:/bin", "HOME": str(self.root), "XDG_RUNTIME_DIR": str(self.runtime)},
            start_new_session=True,
        )
        self.addCleanup(lambda: (daemon.terminate(), daemon.wait(5), daemon.stdout.close()))
        # dbus-daemon prints its address only once it listens; the socket
        # file exists a moment earlier, at bind().
        ready, _, _ = select.select([daemon.stdout], [], [], 10)
        self.assertTrue(ready, "dbus-daemon never listened")
        self.assertTrue(daemon.stdout.readline().startswith(b"unix:path="), "dbus-daemon exited")
        self.assertTrue(socket.exists())
        # An empty address counts as unset: both find the bus through XDG_RUNTIME_DIR.
        for bus in (None, ""):
            with self.subTest(bus=bus):
                code, reply = self.pick("Title", "Choose", bus=bus)
                self.assertEqual((code, reply["ok"], reply["code"]), (1, False, "PORTAL_UNAVAILABLE"))


if __name__ == "__main__":
    unittest.main()
