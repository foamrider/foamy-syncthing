"""How the plugin starts and credits its native core (the core's own
behaviour is tests/test_stream.py)."""

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
PIN = ROOT / "native/native.pin"
COMMIT = "7318f1404e765f274df3d079e35d0ad32b5c1701"


class CoreLaunchTests(unittest.TestCase):
    def test_core_is_started_without_the_desktop_bridge(self):
        service = (ROOT / "Service.qml").read_text()
        arguments = service[
            service.index("startupArguments: [") : service.index("]", service.index("startupArguments: ["))
        ]
        self.assertNotIn('"--desktop-authorized"', arguments)
        self.assertNotRegex(service, r'"webui\.(open|set-theme)"')

    def test_core_and_scan_run_the_bundled_helper_through_foamy_native(self):
        core = (ROOT / "service/CoreProcess.qml").read_text()
        service = (ROOT / "Service.qml").read_text()
        self.assertIn('readonly property string helperName: "foamy-syncthing-helper"', core)
        self.assertIn('FoamyNative.command(root.pluginRoot, root.helperName, ["stream"]', core)
        self.assertIn('FoamyNative.command(root.pluginRoot, "foamy-syncthing-helper", ["conflicts"])', service)
        for source in (core, service):
            self.assertNotRegex(source, r"python3|syncshell-core|/usr/bin/(uname|find)")

    def test_both_architectures_are_pinned(self):
        pin = PIN.read_text()
        self.assertRegex(pin, r"(?m)^binary foamy-syncthing-helper$")
        for arch in ("x86_64", "aarch64"):
            self.assertRegex(pin, rf"(?m)^[0-9a-f]{{64}}  bin/{arch}/foamy-syncthing-helper$")
            self.assertTrue((ROOT / "bin" / arch / "foamy-syncthing-helper").is_file())

    def test_the_port_credits_syncshell(self):
        self.assertIn("Copyright", (ROOT / "LICENSE-SYNCSHELL").read_text())
        readme = (ROOT / "README.md").read_text()
        source = (ROOT / "native/src/main.rs").read_text() + (ROOT / "native/src/core/mod.rs").read_text()
        for text in (readme, source):
            self.assertIn("https://github.com/omarchy-QOL/syncshell", text)
            self.assertIn(COMMIT, text)
        self.assertTrue(re.search(r"LICENSE-SYNCSHELL", readme))


if __name__ == "__main__":
    unittest.main()
