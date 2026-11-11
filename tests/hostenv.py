"""The environment for a Quickshell the tests start.

An AppImage that launched the test run (an editor's terminal) puts its bundled
libraries ($APPDIR/usr/lib) on LD_LIBRARY_PATH; the shell under test must load
the system's. Every other variable is kept."""
import os


def library_path():
    """LD_LIBRARY_PATH without an AppImage's bundled libraries ($APPDIR)."""
    appdir = (os.environ.get("APPDIR") or "").rstrip("/")
    return ":".join(p for p in os.environ.get("LD_LIBRARY_PATH", "").split(":")
                    if p and not (appdir and (p == appdir or p.startswith(appdir + "/"))))


def shell_env(**overrides):
    """os.environ with LD_LIBRARY_PATH cleaned, then `overrides`."""
    env = dict(os.environ, LD_LIBRARY_PATH=library_path())
    if not env["LD_LIBRARY_PATH"]:
        del env["LD_LIBRARY_PATH"]
    env.update(overrides)
    return env
