"""The bundled native core matches its checksum and pinned build metadata."""

from pathlib import Path
import re
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
PIN = dict(line.split("=", 1) for line in (ROOT / "packaging/bundled/core.pin").read_text().splitlines()
           if line and not line.startswith("#"))


class CoreProvenanceTests(unittest.TestCase):
    def test_verify_script_passes(self):
        if not shutil.which("file"):
            self.skipTest("file(1) is unavailable")
        result = subprocess.run([str(ROOT / "packaging/bundled/verify-core.sh")], capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_pin_is_complete_and_consistent(self):
        self.assertRegex(PIN["commit"], r"^[0-9a-f]{40}$")
        self.assertEqual(PIN["tag"], "v" + PIN["version"])
        self.assertRegex(PIN["go"], r"^go\d+\.\d+\.\d+$")
        self.assertTrue(PIN["upstream"].startswith("https://github.com/omarchy-QOL/syncshell"))
        sums = (ROOT / "packaging/bundled/SHA256SUMS").read_text().split()
        self.assertEqual(sums[1], "bin/x86_64/syncshell-core")
        self.assertRegex(sums[0], r"^[0-9a-f]{64}$")

    def test_readme_documents_the_pin(self):
        readme = (ROOT / "README.md").read_text()
        sha = (ROOT / "packaging/bundled/SHA256SUMS").read_text().split()[0]
        for value in (PIN["commit"], PIN["tag"], PIN["go"], sha, "-trimpath", "-buildvcs=false"):
            self.assertIn(value, readme)

    def test_core_is_started_without_the_desktop_bridge(self):
        service = (ROOT / "Service.qml").read_text()
        arguments = service[service.index("startupArguments: ["):service.index("]", service.index("startupArguments: ["))]
        self.assertNotIn('"--desktop-authorized"', arguments)
        self.assertNotRegex(service, r'"webui\.(open|set-theme)"')


if __name__ == "__main__":
    unittest.main()
