"""Exercise stdin delivery, private D-Bus transport, and the real QML queue."""

import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "scripts/syncthing-notify.py"
spec = importlib.util.spec_from_file_location("syncthing_notify", HELPER)
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


def payload(body="Paused private folder", timeout=-1):
    return json.dumps({"summary": "Syncthing", "body": body,
                       "expireTimeout": timeout}, ensure_ascii=False).encode()


def generic_request(language):
    # Evaluate the production adapter method, keeping even fixture text off argv.
    source = r'''
const fs = require('node:fs');
const vm = require('node:vm');
const translations = vm.createContext({});
vm.runInContext(fs.readFileSync('Translations.js', 'utf8'), translations);
const source = fs.readFileSync('hosts/omarchy/OmarchyService.qml', 'utf8');
const start = source.indexOf('  function notify(');
const end = source.indexOf('\n  }', start) + 4;
let request;
const context = vm.createContext({Translations: translations,
  settings: {language: process.env.TEST_NOTIFICATION_LANGUAGE},
  notificationSender: {send(body, expireTimeout) {
    request = {summary: 'Syncthing', body, expireTimeout};
    return true;
  }}});
vm.runInContext(source.slice(start, end), context);
context.notify('Paused private-Canary-847 📁 at /private/docs on PrivateDevice', 10000);
process.stdout.write(JSON.stringify(request));
'''
    result = subprocess.run([shutil.which("node"), "-"], input=source, text=True,
                            cwd=ROOT, env=dict(os.environ, TEST_NOTIFICATION_LANGUAGE=language),
                            capture_output=True, check=True, timeout=3)
    return json.loads(result.stdout)


class NotificationTests(unittest.TestCase):
    def test_utf8_and_expiration_are_preserved(self):
        for timeout in (-1, 0, 10000, 2147483647):
            request = helper.read_request(io.BytesIO(payload('Blåbær 📁 "--exec"\n', timeout)))
            self.assertEqual(request["body"], 'Blåbær 📁 "--exec"\n')
            self.assertEqual(request["expireTimeout"], timeout)

    def test_invalid_and_oversized_input_is_rejected(self):
        invalid = [b"invalid", b"\xff", b"[]", b"{}", payload("nul\x00"),
                   payload(timeout=True), payload(timeout=-2), payload(timeout=2147483648),
                   payload("x" * 32768),
                   b'{"summary":"x","body":"\\ud800","expireTimeout":-1}',
                   b'{"summary":"x","body":7,"expireTimeout":-1}']
        for raw in invalid:
            with self.subTest(raw=raw[:40]), self.assertRaises((ValueError, UnicodeError)):
                helper.read_request(io.BytesIO(raw))

    def test_transport_errors_do_not_echo_private_text(self):
        env = dict(os.environ, DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent-foamy-test-bus")
        result = subprocess.run(["/usr/bin/python3", str(HELPER)], input=payload("private-Canary-847"),
                                env=env, capture_output=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
        self.assertEqual(result.stderr, b"Desktop notification delivery failed.\n")

    def test_stalled_stdin_has_a_total_deadline(self):
        started = time.monotonic()
        with subprocess.Popen(["/usr/bin/python3", str(HELPER)], stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
            process.wait(timeout=5)
            _, error = process.communicate()
        self.assertLess(time.monotonic() - started, 4)
        self.assertEqual(error, b"Desktop notification delivery failed.\n")

    def test_real_dbus_delivery_keeps_text_out_of_process_arguments(self):
        result = subprocess.run(["dbus-run-session", "--", "/usr/bin/python3", str(Path(__file__).resolve()),
                                 "--bus-fixture"], capture_output=True, text=True, timeout=12)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("NOTIFICATION_BUS passed", result.stdout)

    def test_real_receiver_persistence_never_receives_private_metadata(self):
        result = subprocess.run(["dbus-run-session", "--", "/usr/bin/python3", str(Path(__file__).resolve()),
                                 "--receiver-fixture"], capture_output=True, text=True, timeout=12)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("NOTIFICATION_RECEIVER passed", result.stdout)

    def test_qml_notification_queue(self):
        runner = shutil.which("qs")
        if not runner:
            self.skipTest("Quickshell is unavailable")
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            state = base / "delivered.jsonl"
            mock = base / "notify.py"
            mock.write_text('''import json, os, sys, time
from pathlib import Path
request = json.load(sys.stdin)
if request["body"] == "stall": time.sleep(30)
if request["body"] == "fail": sys.exit(1)
with Path(os.environ["TEST_NOTIFICATION_STATE"]).open("a") as output:
    output.write(json.dumps({"request":request, "argv":sys.argv,
        "cmdline":Path("/proc/self/cmdline").read_bytes().decode()}) + "\\n")
''')
            fixture = base / "shell.qml"
            fixture.write_text('''import QtQuick
import Quickshell
ShellRoot {
  FloatingWindow {
    visible: true
    implicitWidth: 300; implicitHeight: 100
    Loader { anchors.fill: parent; source: %s }
  }
}
''' % json.dumps(str(ROOT / "tests/tst_notifications.qml")))
            runtime = base / "runtime"
            runtime.mkdir(mode=0o700)
            env = dict(os.environ, QT_QPA_PLATFORM="offscreen", QT_QUICK_BACKEND="software",
                       QT_QPA_PLATFORMTHEME="", XDG_RUNTIME_DIR=str(runtime),
                       TEST_NOTIFICATION_HELPER=str(mock), TEST_NOTIFICATION_STATE=str(state))
            result = subprocess.run([runner, "-p", str(fixture), "--no-color"], env=env,
                                    text=True, capture_output=True, timeout=20)
            output = result.stdout + result.stderr
            self.assertEqual(result.returncode, 0, output)
            self.assertRegex(output, r"NOTIFICATION_QUEUE passed=[1-9][0-9]* failed=0")
            records = [json.loads(line) for line in state.read_text().splitlines()]
            bodies = [record["request"]["body"] for record in records]
            self.assertIn("private-Canary-847 📁", bodies)
            self.assertIn("after timeout", bodies)
            self.assertIn("after failure", bodies)
            for record in records:
                self.assertEqual(record["argv"], [str(mock)])
                self.assertNotIn("private-Canary-847", record["cmdline"])
            print(output[output.index("NOTIFICATION_QUEUE"):].splitlines()[0])


def bus_fixture(receiver=False):
    import dbus
    import dbus.service
    from dbus.mainloop.glib import DBusGMainLoop
    from gi.repository import GLib

    DBusGMainLoop(set_as_default=True)
    received = threading.Event()
    release = threading.Event()
    records = []
    writers = []
    directory = tempfile.TemporaryDirectory()
    persisted = Path(directory.name) / "popup.json"
    bus = dbus.SessionBus()
    name = dbus.service.BusName("org.freedesktop.Notifications", bus=bus)

    class Server(dbus.service.Object):
        @dbus.service.method("org.freedesktop.Notifications", in_signature="susssasa{sv}i", out_signature="u")
        def Notify(self, app, replaces, icon, summary, body, actions, hints, expiration):
            records.append((str(summary), str(body), int(expiration), int(hints["urgency"])))
            if receiver:
                # Match the supported receiver's JSON-in-bash-argv persistence.
                snapshot = json.dumps({"summary": str(summary), "body": str(body)}, ensure_ascii=False)
                writers.append(subprocess.Popen([
                    "bash", "-c", 'printf "%s\\n" "$1" > "$2"; read -r ignored',
                    "--", snapshot, str(persisted)], stdin=subprocess.PIPE,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE))
            received.set()
            release.wait(1)
            return dbus.UInt32(23)

    server = Server(name, "/org/freedesktop/Notifications")
    loop = GLib.MainLoop()
    thread = threading.Thread(target=loop.run, daemon=True)
    thread.start()
    try:
        requests = [generic_request(language) for language in ("en", "nb")] if receiver else [
            json.loads(payload('Paused private-Canary-847 📁 "--exec"\nLocal only', 10000))]
        for request in requests:
            received.clear()
            release.clear()
            with subprocess.Popen(["/usr/bin/python3", str(HELPER)], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
                process.stdin.write(json.dumps(request, ensure_ascii=False).encode())
                process.stdin.close()
                process.stdin = None
                assert received.wait(2), "D-Bus server did not receive notification"
                command = Path(f"/proc/{process.pid}/cmdline").read_bytes()
                children = Path(f"/proc/{process.pid}/task/{process.pid}/children").read_text().strip()
                assert b"private-Canary-847" not in command, "private text leaked to sender argv"
                assert children == "", "notification sender launched a subprocess"
                if receiver:
                    writer = writers[-1]
                    argv = Path(f"/proc/{writer.pid}/cmdline").read_bytes()
                    for private in (b"private-Canary-847", b"/private/docs", b"PrivateDevice"):
                        assert private not in argv, "private metadata leaked to receiver argv"
                    writer.communicate(input=b"\n", timeout=2)
                    entry = json.loads(persisted.read_text())
                    assert entry == {"summary": "Syncthing", "body": request["body"]}, entry
                    assert "private-Canary-847" not in persisted.read_text()
                release.set()
                stdout, stderr = process.communicate(timeout=3)
                assert process.returncode == 0, stderr
                assert stdout == b"" and stderr == b"", "notification data leaked to output"
        assert records == [("Syncthing", request["body"], 10000, 0) for request in requests], records
        print("NOTIFICATION_RECEIVER passed" if receiver else "NOTIFICATION_BUS passed")
    finally:
        release.set()
        loop.quit()
        thread.join(timeout=2)
        server.remove_from_connection()
        for writer in writers:
            if writer.poll() is None:
                writer.kill()
                writer.communicate(timeout=2)
        directory.cleanup()


if __name__ == "__main__":
    if sys.argv[1:] == ["--bus-fixture"]:
        bus_fixture()
    elif sys.argv[1:] == ["--receiver-fixture"]:
        bus_fixture(receiver=True)
    else:
        unittest.main()
