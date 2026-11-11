"""The Quickshell the tests start loads the system libraries, not an AppImage's."""
import os
import unittest
from unittest import mock

import hostenv


class HostEnvTests(unittest.TestCase):
    def test_appimage_libraries_are_dropped(self):
        with mock.patch.dict(os.environ, {"APPDIR": "/tmp/.mount_App/",
                                          "LD_LIBRARY_PATH": "/tmp/.mount_App/usr/lib::/opt/lib:/tmp/.mount_Appx"}):
            env = hostenv.shell_env(QT_QPA_PLATFORM="offscreen")
        self.assertEqual(env["LD_LIBRARY_PATH"], "/opt/lib:/tmp/.mount_Appx")
        self.assertEqual(env["QT_QPA_PLATFORM"], "offscreen")

    def test_only_appimage_entries_leave_nothing(self):
        with mock.patch.dict(os.environ, {"APPDIR": "/tmp/.mount_App", "LD_LIBRARY_PATH": "/tmp/.mount_App/usr/lib"}):
            self.assertNotIn("LD_LIBRARY_PATH", hostenv.shell_env())
        with mock.patch.dict(os.environ, {"LD_LIBRARY_PATH": "/opt/lib"}):
            os.environ.pop("APPDIR", None)
            self.assertEqual(hostenv.shell_env()["LD_LIBRARY_PATH"], "/opt/lib")


if __name__ == "__main__":
    unittest.main()
