"""The native core is a port of Syncshell: its MIT credit and the ported commit
stay in LICENSE-SYNCSHELL, README and the crate documentation (the core's own
behaviour is tests/test_stream.py)."""

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
COMMIT = "7318f1404e765f274df3d079e35d0ad32b5c1701"


class CoreCreditTests(unittest.TestCase):
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
