"""Runs the native core (`stream`) as a process for the black-box tests, with
a temporary HOME, a fake Syncthing (tests/fake_syncthing.py), and fake
`systemctl` and `syncthing` executables on a PATH that holds nothing else.

FOAMY_HELPER: JSON argv prefix of the helper binary; the core runs as
`<prefix> stream <flags>`. The default is this machine's bundled helper,
bin/<machine>/foamy-syncthing-helper (bin/aarch64/… also runs here through
qemu binfmt)."""

import json
import os
import unittest
import platform
import queue
import shutil
import subprocess
import tempfile
import threading
import time
from pathlib import Path

import fake_syncthing

ROOT = Path(__file__).resolve().parents[1]
UNIT = "syncthing.service"
PLUGIN_FLAGS = ["--probe-interval-seconds", "15", "--desired-service-state", "enabled",
                "--lifecycle-kind", "systemd-user", "--lifecycle-authorized", "--lifecycle-unit", UNIT]


def helper_prefix():
    if os.environ.get("FOAMY_HELPER"):
        return [str(Path(part).resolve()) if i == 0 and not os.path.isabs(part) else part
                for i, part in enumerate(json.loads(os.environ["FOAMY_HELPER"]))]
    return [str(ROOT / "bin" / platform.machine() / "foamy-syncthing-helper")]


SYSTEMCTL = r'''#!/usr/bin/python3 -I
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_CALLS"], "a") as log:
    log.write(json.dumps(["systemctl"] + args) + "\n")
path = os.environ["FAKE_SYSTEMCTL_STATE"]
with open(path) as source:
    state = json.load(source)
if state.get("fail"):
    sys.exit(1)
if args[:2] == ["--user", "show"]:
    for key in ("LoadState", "ActiveState", "UnitFileState", "FragmentPath", "ExecStart", "Environment"):
        print(key + "=" + state.get(key, ""))
elif len(args) == 3 and args[0] == "--user" and args[1] in ("start", "stop"):
    if state.get("fail_action"):
        print("Failed to " + args[1] + " unit: private-Canary-847", file=sys.stderr)
        sys.exit(1)
    state["ActiveState"] = "active" if args[1] == "start" else "inactive"
    with open(path, "w") as output:
        json.dump(state, output)
else:
    sys.exit(2)
'''

SYNCTHING = r'''#!/usr/bin/python3 -I
import json, os, sys
with open(os.environ["FAKE_CALLS"], "a") as log:
    log.write(json.dumps(["syncthing"] + sys.argv[1:]) + "\n")
if sys.argv[1:] == ["paths"]:
    print("Configuration file:\n\t" + os.environ.get("FAKE_SYNCTHING_CONFIG", "") + "\n")
'''


# Runs argv[3:] in the user, mount and network namespace `unshare -rmn` made
# for it, where loopback is up and /etc/resolv.conf (bind-mounted from
# argv[2]) names one DNS server, on loopback, that takes queries and never
# answers. argv[1] is the `ip` binary. Nothing outside the namespace changes.
SILENT_DNS = r'''#!/usr/bin/python3 -I
import os, socket, subprocess, sys
ip, resolv = sys.argv[1], sys.argv[2]
subprocess.run([ip, "link", "set", "lo", "up"], check=True)
subprocess.run(["/usr/bin/mount", "--bind", resolv, "/etc/resolv.conf"], check=True)
server = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
server.bind(("127.0.0.1", 53))
os.set_inheritable(server.fileno(), True)
os.execv(sys.argv[3], sys.argv[3:])
'''
# The resolver would wait 30 s for an answer (musl and Go read these options).
SILENT_RESOLV = "nameserver 127.0.0.1\noptions timeout:30 attempts:1\n"


def silent_dns_prefix(directory):
    """The argv prefix that runs a command with a DNS server that never
    answers (see SILENT_DNS), or None where unprivileged namespaces, `ip`,
    `mount` or /etc/resolv.conf are missing."""
    unshare, ip = shutil.which("unshare"), shutil.which("ip")
    if not (unshare and ip and os.path.exists("/usr/bin/mount") and os.path.exists("/etc/resolv.conf")):
        return None
    directory = Path(directory)
    (directory / "silent-dns.py").write_text(SILENT_DNS)
    (directory / "resolv.conf").write_text(SILENT_RESOLV)
    prefix = [unshare, "--user", "--map-root-user", "--mount", "--net", "/usr/bin/python3", "-I",
              str(directory / "silent-dns.py"), ip, str(directory / "resolv.conf")]
    try:
        probe = subprocess.run(prefix + ["/usr/bin/true"], capture_output=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return prefix if probe.returncode == 0 else None


def proc_argv(pid):
    """/proc/<pid>/cmdline as a list, without a binfmt emulator's prefix
    (qemu-aarch64-static <binary> <argv...>)."""
    argv = Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")[:-1]
    if argv and os.path.basename(argv[0]).startswith(b"qemu-"):
        argv = argv[2:]
    return argv


class Core:
    """One core process against one fake Syncthing."""

    def __init__(self, tls=False, api_key=fake_syncthing.API_KEY, flags=None, config=True,
                 systemctl=None, start=True, gui_address=None, unix_socket=None):
        self.temp = tempfile.TemporaryDirectory(prefix="foamy-syncthing-core-")
        self.home = Path(self.temp.name)
        self.bin = self.home / "bin"
        self.bin.mkdir()
        self.calls_path = self.home / "calls.jsonl"
        self.calls_path.touch()
        self.systemctl_path = self.home / "systemctl.json"
        for name, source in (("systemctl", SYSTEMCTL), ("syncthing", SYNCTHING)):
            (self.bin / name).write_text(source)
            (self.bin / name).chmod(0o755)
        self.config_dir = self.home / "state" / "syncthing"
        self.config_dir.mkdir(parents=True)
        self.config_path = self.config_dir / "config.xml"
        certificate = None
        if tls:
            certificate = fake_syncthing.make_certificate(str(self.config_dir))
            if certificate is None:
                raise unittest.SkipTest("openssl is unavailable")
        # unix_socket: a file name in the temporary HOME for the fake's socket.
        self.syncthing = fake_syncthing.FakeSyncthing(
            self.home, tls=certificate, unix=str(self.home / unix_socket) if unix_socket else None)
        self.syncthing.state["paths"]["config"] = str(self.config_path)
        if config:
            self.write_config(tls=tls, api_key=api_key, address=gui_address or self.syncthing.address)
        self.set_systemctl(systemctl or {})
        self.env = {"HOME": str(self.home), "XDG_CONFIG_HOME": str(self.home / "config"),
                    "XDG_STATE_HOME": str(self.home / "state"), "PATH": str(self.bin),
                    "FAKE_CALLS": str(self.calls_path), "FAKE_SYSTEMCTL_STATE": str(self.systemctl_path),
                    "LANG": "C.UTF-8"}
        self.flags = PLUGIN_FLAGS if flags is None else flags
        # An argv prefix the core runs under (silent_dns_prefix).
        self.wrapper = []
        self.frames = []
        self.lines = queue.Queue()
        self.process = None
        if start:
            self.start()

    def write_config(self, tls=False, api_key=fake_syncthing.API_KEY, address=None):
        self.config_path.write_text(
            '<configuration version="37">\n'
            f'  <gui enabled="true" tls="{"true" if tls else "false"}" debugging="false">\n'
            f'    <address>{address}</address>\n'
            f'    <apikey>{api_key}</apikey>\n'
            '    <theme>default</theme>\n'
            '  </gui>\n'
            '</configuration>\n')

    def set_systemctl(self, overrides):
        state = {"LoadState": "loaded", "ActiveState": "active", "UnitFileState": "enabled",
                 "FragmentPath": "/usr/lib/systemd/user/syncthing.service",
                 "ExecStart": "{ path=/usr/bin/syncthing ; argv[]=/usr/bin/syncthing serve --no-browser --no-restart ; "
                              "ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }",
                 "Environment": "STNORESTART=1"}
        state.update(overrides)
        self.systemctl_path.write_text(json.dumps(state))

    def systemctl_state(self):
        return json.loads(self.systemctl_path.read_text())

    def argv(self):
        return self.wrapper + helper_prefix() + ["stream"] + self.flags

    def start(self):
        self.process = subprocess.Popen(self.argv(), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, env=self.env, cwd=str(self.home))
        self.stderr = b""
        self.started = time.monotonic()
        threading.Thread(target=self._read, daemon=True).start()
        self.stderr_reader = threading.Thread(target=self._read_stderr, daemon=True)
        self.stderr_reader.start()

    def _read(self):
        # (arrival time, line): a frame's timing doesn't depend on when the
        # test gets round to reading it.
        for line in self.process.stdout:
            self.lines.put((time.monotonic(), line))
        self.lines.put((time.monotonic(), None))

    def _read_stderr(self):
        self.stderr = self.process.stderr.read()

    def next_frame(self, timeout=10):
        arrived, line = self.lines.get(timeout=timeout)
        if line is None:
            raise EOFError("core output ended")
        self.raw = getattr(self, "raw", b"") + line
        frame = json.loads(line)
        self.frames.append(frame)
        self.arrived = arrived
        return frame

    def wait_for(self, predicate, timeout=10, what="frame"):
        until = time.monotonic() + timeout
        while True:
            left = until - time.monotonic()
            if left <= 0:
                raise AssertionError(f"timed out waiting for {what}; frames: {self.frames[-5:]}")
            try:
                frame = self.next_frame(timeout=left)
            except queue.Empty:
                continue
            if predicate(frame):
                return frame

    def snapshot(self, predicate=lambda state: True, timeout=10, what="snapshot"):
        return self.wait_for(lambda f: f["type"] == "snapshot" and predicate(f["state"]), timeout, what)

    def ready(self, timeout=10):
        hello = self.next_frame(timeout)
        assert hello["type"] == "hello", hello
        return self.snapshot(lambda s: s["connection"]["phase"] in ("ready", "error"), timeout)

    def send(self, message):
        line = message if isinstance(message, (bytes, str)) else json.dumps(message)
        if isinstance(line, str):
            line = line.encode()
        self.process.stdin.write(line + (b"" if line.endswith(b"\n") else b"\n"))
        self.process.stdin.flush()

    def request(self, kind, request_id, timeout=20, **values):
        self.send(dict({"v": 2, "type": kind, "id": request_id}, **values))
        return self.wait_for(lambda f: f["type"] in ("result", "fatal") and f.get("id", request_id) == request_id,
                             timeout, f"result {request_id}")

    def action(self, name, request_id, args=None, timeout=20):
        return self.request("action", request_id, timeout=timeout, action=name, args={} if args is None else args)

    def latest_state(self):
        for frame in reversed(self.frames):
            if frame["type"] == "snapshot":
                return frame["state"]
        return None

    def calls(self):
        return [json.loads(line) for line in self.calls_path.read_text().splitlines()]

    def finish(self, timeout=10):
        try:
            self.process.stdin.close()
        except BrokenPipeError:
            pass
        code = self.process.wait(timeout=timeout)
        # stderr is complete only once its reader has seen EOF.
        self.stderr_reader.join(timeout=timeout)
        while True:
            try:
                _, line = self.lines.get(timeout=2)
            except queue.Empty:
                break
            if line is None:
                break
            self.raw = getattr(self, "raw", b"") + line
            self.frames.append(json.loads(line))
        return code

    def close(self):
        if self.process and self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=5)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr) if self.process else ():
            try:
                stream.close()
            except Exception:
                pass
        self.syncthing.close()
        self.temp.cleanup()

