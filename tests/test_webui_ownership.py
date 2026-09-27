"""Exercise real Web UI scripts with temporary assets and mocked native removal."""

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
MARKER = ".foamy-syncthing-webui"
OWNER = "foamy.syncthing:webui:v1\n"


def snapshot(root):
    result = {}
    for path in sorted(root.rglob("*")):
        name = str(path.relative_to(root))
        if path.is_symlink():
            result[name] = ("link", os.readlink(path))
        elif path.is_file():
            result[name] = ("file", hashlib.sha256(path.read_bytes()).hexdigest())
        else:
            result[name] = ("directory",)
    return result


class WebUiOwnershipTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.assets = self.base / "gui assets"
        self.assets.mkdir()
        self.bin = self.base / "bin"
        self.bin.mkdir()
        self.env = dict(os.environ, HOME=str(self.base / "home"),
                        XDG_CONFIG_HOME=str(self.base / "config"),
                        XDG_STATE_HOME=str(self.base / "state"),
                        XDG_RUNTIME_DIR=str(self.base / "runtime"),
                        PATH=f"{self.bin}:{os.environ['PATH']}",
                        TEST_ASSETS=str(self.assets),
                        TEST_DONE=str(self.base / "done"))
        self.mock("omarchy-theme-color", """
for key in background foreground accent muted selection lighter_background darker_background dark_foreground light_foreground red yellow green cyan blue magenta orange; do
  printf '%s\t#123456\n' "$key"
done
""")

    def mock(self, name, body):
        path = self.bin / name
        path.write_text("#!/bin/bash\nset -euo pipefail\n" + body)
        path.chmod(0o755)

    def run_script(self, *args, success=True):
        result = subprocess.run(["bash", *map(str, args)], env=self.env,
                                text=True, capture_output=True, timeout=15)
        if success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def prepare(self, method, success=True):
        if method == "install":
            return self.run_script(ROOT / "webui/install.sh", self.assets,
                                   success=success)
        return self.run_script(ROOT / "hosts/omarchy/scripts/syncthing-theme.sh",
                               "prepare", method, self.assets, success=success)

    def target(self, method):
        return self.assets / ("syncthing-omarchy" if method == "omarchy"
                              else "syncshell-modern")

    def fixture(self, method, kind):
        target = self.target(method)
        if kind == "file":
            target.write_text("unrelated file")
            return target
        outside = self.assets / "unrelated"
        if kind in ("link", "dangling-link"):
            if kind == "link":
                outside.mkdir()
                (outside / "keep").write_text("unrelated content")
                (outside / MARKER).write_text(OWNER)
            target.symlink_to(outside, target_is_directory=True)
            return target
        target.mkdir()
        (target / "keep").write_text("original content")
        if kind == "owned":
            (target / MARKER).write_text(OWNER)
        elif kind == "wrong-marker":
            (target / MARKER).write_text("another.plugin:webui:v1\n")
        elif kind == "linked-marker":
            outside.write_text(OWNER)
            (target / MARKER).symlink_to(outside)
        elif kind == "legacy":
            revision = hashlib.sha256((ROOT / "webui/SHA256SUMS").read_bytes()).hexdigest()
            (target / ".syncshell-bundle").write_text(revision + "\n")
        return target

    def clear_assets(self):
        shutil.rmtree(self.assets)
        self.assets.mkdir()

    def test_setup_preserves_unowned_paths(self):
        for method in ("install", "modern", "omarchy"):
            for kind in ("unmarked", "legacy", "wrong-marker", "linked-marker",
                         "file", "link", "dangling-link"):
                with self.subTest(method=method, kind=kind):
                    self.clear_assets()
                    self.fixture(method, kind)
                    before = snapshot(self.assets)
                    result = self.prepare(method, success=False)
                    self.assertIn("Refusing to replace unowned Web UI path", result.stderr)
                    self.assertEqual(snapshot(self.assets), before)

    def test_fresh_install_and_owned_upgrade(self):
        for method in ("install", "modern", "omarchy"):
            with self.subTest(method=method):
                self.clear_assets()
                self.prepare(method)
                target = self.target(method)
                self.assertEqual((target / MARKER).read_text(), OWNER)
                self.assertTrue((target / "index.html").is_file())
                (target / "obsolete").write_text("old bundle content")
                (target / ".syncshell-bundle").write_text("old revision\n")
                self.prepare(method)
                self.assertFalse((target / "obsolete").exists())
                self.assertEqual((target / MARKER).read_text(), OWNER)
                self.assertEqual(list(self.assets.iterdir()), [target])
                self.prepare(method)  # Also exercise refresh without replacing the bundle.
                self.assertEqual((target / MARKER).read_text(), OWNER)

    def test_failed_replacement_restores_owned_directory(self):
        real_mv = shutil.which("mv")
        self.env["TEST_REAL_MV"] = real_mv
        self.mock("mv", """
source_path=$2
if [[ $(dirname -- "$source_path") == "$TEST_ASSETS"
    && $source_path == */.syncshell-modern.* && $source_path != *.previous ]]; then
  [[ -d $source_path.previous ]] || exit 72
  exit 71
fi
exec "$TEST_REAL_MV" "$@"
""")
        for method in ("install", "modern"):
            with self.subTest(method=method):
                self.clear_assets()
                self.fixture(method, "owned")
                before = snapshot(self.assets)
                result = self.prepare(method, success=False)
                self.assertEqual(result.returncode, 71, result.stderr)
                self.assertEqual(snapshot(self.assets), before)

    def remove(self, mode, drop_marker=False):
        installed = Path(self.env["XDG_CONFIG_HOME"]) / "omarchy/plugins/foamy.syncthing"
        scripts = installed / "hosts/omarchy/scripts"
        scripts.mkdir(parents=True)
        (installed / "webui").mkdir()
        shutil.copy2(ROOT / "manifest.json", installed)
        shutil.copy2(ROOT / "hosts/omarchy/scripts/syncthing-remove.sh", scripts)
        shutil.copy2(ROOT / "webui/ownership.sh", installed / "webui")
        self.env.update(TEST_INSTALLED=str(installed), TEST_ASSETS=str(self.assets),
                        TEST_DROP_MARKER="yes" if drop_marker else "no")
        self.mock("omarchy", """
[[ $* == 'plugin remove foamy.syncthing --yes' ]]
rm -rf -- "$TEST_INSTALLED"
if [[ $TEST_DROP_MARKER == yes ]]; then
  rm -- "$TEST_ASSETS/syncshell-modern/.foamy-syncthing-webui"
fi
""")
        self.mock("omarchy-notification-send", 'printf "%s\\n" "$@" >"$TEST_DONE"\n')
        settings = Path(self.env["XDG_CONFIG_HOME"]) / "omarchy/foamy.syncthing"
        settings.mkdir(parents=True, exist_ok=True)
        (settings / "settings.toml").write_text("keep settings")
        self.run_script(scripts / "syncthing-remove.sh", "start", installed,
                        self.assets, mode)
        workers = Path(self.env["XDG_RUNTIME_DIR"]) / "omarchy-foamy.syncthing"
        # Wait for the detached worker's notification and its EXIT cleanup.
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if Path(self.env["TEST_DONE"]).exists() and not list(workers.glob("remove.*")):
                break
            time.sleep(0.02)
        else:
            self.fail("Removal worker did not finish")
        self.assertFalse(installed.exists())
        self.assertEqual(settings.exists(), mode == "preserve")
        return Path(self.env["TEST_DONE"]).read_text()

    def test_removal_preserves_unowned_paths_and_deletes_owned_paths(self):
        for mode in ("preserve", "purge"):
            for kind in ("unmarked", "legacy", "wrong-marker", "linked-marker",
                         "file", "link", "dangling-link", "absent"):
                with self.subTest(mode=mode, kind=kind):
                    self.clear_assets()
                    self.fixture("omarchy", "owned")
                    if kind != "absent":
                        self.fixture("modern", kind)
                    before = {k: v for k, v in snapshot(self.assets).items()
                              if not k.startswith("syncthing-omarchy")}
                    message = self.remove(mode)
                    self.assertEqual(snapshot(self.assets), before)
                    if kind != "absent":
                        self.assertIn("unowned Web UI paths preserved", message)
                    Path(self.env["TEST_DONE"]).unlink()

    def test_removal_rechecks_ownership_after_native_removal(self):
        target = self.fixture("modern", "owned")
        self.remove("preserve", drop_marker=True)
        self.assertEqual((target / "keep").read_text(), "original content")
        self.assertFalse((target / MARKER).exists())


if __name__ == "__main__":
    unittest.main()
