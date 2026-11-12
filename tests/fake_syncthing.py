"""A loopback stand-in for Syncthing's REST API, for the native core's
black-box tests (tests/test_stream.py). Never touches a real Syncthing.

The server answers the endpoints the core uses from one in-memory `state`
dict, applies the mutations it receives (PATCH/POST/DELETE folders, scans),
and records every request (method, raw target, whether an API key came,
body). With `tls=True` it serves HTTPS with a throwaway certificate made by
`openssl` (see `make_certificate`); with `unix=<path>` it listens on a Unix
socket at that path instead of a loopback port."""

import copy
import json
import os
import shutil
import socket
import socketserver
import ssl
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, unquote, urlsplit

API_KEY = "test-key-Canary-4711"
LOCAL_ID = "AAAAAAA-BBBBBBB-CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG-HHHHHHH"
PEER_ID = "BBBBBBB-CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG-HHHHHHH-IIIIIII"
OTHER_ID = "CCCCCCC-DDDDDDD-EEEEEEE-FFFFFFF-GGGGGGG-HHHHHHH-IIIIIII-JJJJJJJ"


def default_state(home):
    return {
        "myID": LOCAL_ID,
        "tilde": str(home),
        "version": "v2.0.10",
        "devices": [
            {"deviceID": PEER_ID, "name": "laptop", "untrusted": False, "addresses": ["dynamic"]},
            {"deviceID": LOCAL_ID, "name": "this", "untrusted": False},
            {"deviceID": OTHER_ID, "name": "<phone & co>", "untrusted": True},
        ],
        "connections": {PEER_ID: {"connected": True}, OTHER_ID: {"connected": False}},
        "folders": [],
        "status": {},
        "errors": {},
        "pendingFolders": {},
        "pendingDevices": {},
        "discovery": {},
        "gui": {"theme": "default", "address": "ignored"},
        "paths": {"guiAssets": "/usr/share/syncthing/gui", "config": "/tmp/config.xml"},
        "defaultFolder": {
            "id": "",
            "label": "",
            "path": "~",
            "type": "sendreceive",
            "rescanIntervalS": 3600,
            "fsWatcherEnabled": True,
            "paused": False,
            "devices": [],
            "minDiskFree": {"value": 1, "unit": "%"},
        },
        "random": "AbCdEfGhIj",
        "events": [],
        "files": {},
    }


def folder(path, folder_id="docs", label="Documents", paused=False, devices=(LOCAL_ID, PEER_ID)):
    return {
        "id": folder_id,
        "label": label,
        "path": str(path),
        "paused": paused,
        "markerName": ".stfolder",
        "type": "sendreceive",
        "devices": [{"deviceID": device, "introducedBy": "", "encryptionPassword": ""} for device in devices],
    }


def make_certificate(directory, name="https"):
    """(cert path, key path) of a fresh self-signed P-256 certificate, or
    None without openssl."""
    if not shutil.which("openssl"):
        return None
    cert = os.path.join(directory, name + "-cert.pem")
    key = os.path.join(directory, name + "-key.pem")
    subprocess.run(
        [
            "openssl",
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-nodes",
            "-subj",
            "/CN=syncthing",
            "-days",
            "2",
            "-addext",
            "subjectAltName=IP:127.0.0.1",
            "-keyout",
            key,
            "-out",
            cert,
        ],
        check=True,
        capture_output=True,
        timeout=30,
    )
    return cert, key


class QuietServer(ThreadingHTTPServer):
    def handle_error(self, request, client_address):
        # Clients that hang up mid-reply (the core's deadlines, TLS refusals).
        pass


class QuietUnixServer(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    def handle_error(self, request, client_address):
        pass


class FullBacklog:
    """A Unix stream listener that never accepts, with its listen backlog
    already full: a connect to it can only wait (a blocking one forever)."""

    def __init__(self, directory):
        self.path = os.path.join(directory, "gui.sock")
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(self.path)
        self.listener.listen(0)
        self.clients = []
        while True:
            client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            client.setblocking(False)
            try:
                client.connect(self.path)
            except BlockingIOError:
                client.close()
                break
            self.clients.append(client)

    def close(self):
        for client in self.clients:
            client.close()
        self.listener.close()


class Trickler:
    """A loopback endpoint that answers every request a byte at a time and
    never finishes: `mode="header"` sends a status line, then one header line
    every `gap` seconds and never the blank line; `mode="body"` sends complete
    headers announcing a large body, then one byte of it every `gap` seconds.
    Each piece arrives well within any per-read socket timeout, so only an
    absolute deadline ends the request. HTTPS with `tls=(cert, key)`."""

    def __init__(self, mode, gap, tls=None):
        self.mode, self.gap = mode, gap
        self.context = None
        if tls:
            self.context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            self.context.load_cert_chain(tls[0], tls[1])
        self.listener = socket.create_server(("127.0.0.1", 0))
        self.port = self.listener.getsockname()[1]
        self.closed = threading.Event()
        self.requests = 0
        threading.Thread(target=self._accept, daemon=True).start()

    @property
    def address(self):
        return f"127.0.0.1:{self.port}"

    def close(self):
        self.closed.set()
        self.listener.close()

    def _accept(self):
        while not self.closed.is_set():
            try:
                connection, _ = self.listener.accept()
            except OSError:
                return
            threading.Thread(target=self._serve, args=(connection,), daemon=True).start()

    def _serve(self, connection):
        try:
            connection.settimeout(10)
            if self.context:
                connection = self.context.wrap_socket(connection, server_side=True)
            head = b""
            while b"\r\n\r\n" not in head:
                data = connection.recv(4096)
                if not data:
                    return
                head += data
            self.requests += 1
            if self.mode == "header":
                connection.sendall(b"HTTP/1.1 200 OK\r\n")
                piece = b"X-Trickle: 1\r\n"
            else:
                connection.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4096\r\n\r\n")
                piece = b" "
            while not self.closed.wait(self.gap):
                connection.sendall(piece)
        except (OSError, ssl.SSLError):
            pass
        finally:
            connection.close()


class FakeSyncthing:
    def __init__(self, home, api_key=API_KEY, tls=None, unix=None):
        self.state = default_state(home)
        self.api_key = api_key
        self.requests = []
        self.lock = threading.Lock()
        self.overrides = {}
        self.delays = {}
        self.down = None
        self.next_event = 1
        handler = self._handler()
        self.unix = unix
        if unix:
            # TCP_NODELAY doesn't apply to a Unix socket.
            handler = type("UnixHandler", (handler,), {"disable_nagle_algorithm": False})
            self.server = QuietUnixServer(unix, handler)
        else:
            self.server = QuietServer(("127.0.0.1", 0), handler)
        self.server.daemon_threads = True
        if tls:
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(tls[0], tls[1])
            self.server.socket = context.wrap_socket(self.server.socket, server_side=True)
        self.port = None if unix else self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()

    @property
    def address(self):
        """`127.0.0.1:<port>`, or the socket's path."""
        return self.unix or f"127.0.0.1:{self.port}"

    def add_event(self, kind, data):
        with self.lock:
            self.state["events"].append(
                {
                    "id": self.next_event,
                    "globalID": self.next_event,
                    "type": kind,
                    "time": "2026-11-06T00:00:00Z",
                    "data": data,
                }
            )
            self.next_event += 1

    def wait_polling(self, timeout=10):
        """Until the core's event loop long-polls (events added before that
        are skipped, as by Syncthing's own clients)."""
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            if any("timeout=1" in call["target"] for call in self.calls("GET", "/rest/events")):
                return
            time.sleep(0.05)
        raise AssertionError("the core never long-polled /rest/events")

    def calls(self, method=None, prefix=""):
        with self.lock:
            return [
                r for r in self.requests if (method is None or r["method"] == method) and r["path"].startswith(prefix)
            ]

    def _handler(self):
        fake = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"
            disable_nagle_algorithm = True

            def log_message(self, *args):
                pass

            def reply(self, status, value=None, raw=None):
                body = raw if raw is not None else (b"" if value is None else json.dumps(value).encode())
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def handle_any(self):
                length = int(self.headers.get("Content-Length") or 0)
                body = self.rfile.read(length) if length else b""
                target = urlsplit(self.path)
                record = {
                    "method": self.command,
                    "path": target.path,
                    "target": self.path,
                    "key": self.headers.get("X-API-Key"),
                    "body": body.decode("utf-8", "replace"),
                    "headers": dict(self.headers.items()),
                }
                with fake.lock:
                    fake.requests.append(record)
                if fake.down and fake.down():
                    # A stopped Syncthing: drop the connection unanswered.
                    self.close_connection = True
                    return
                delay = fake.delays.get(target.path)
                if delay:
                    time.sleep(delay)
                override = fake.overrides.get((self.command, target.path))
                if override is not None:
                    status, payload = override
                    if isinstance(payload, bytes):
                        return self.reply(status, raw=payload)
                    return self.reply(status, payload)
                if target.path == "/rest/noauth/health":
                    return self.reply(200, {"status": "OK"})
                if self.headers.get("X-API-Key") != fake.api_key:
                    return self.reply(403, raw=b"CSRF Error\n")
                query = {k: v[0] for k, v in parse_qs(target.query, keep_blank_values=True).items()}
                with fake.lock:
                    result = fake.route(self.command, target.path, query, body)
                if result is None:
                    return self.reply(404, raw=b"404 page not found\n")
                status, payload = result
                if self.command == "GET" and target.path == "/rest/events" and payload == []:
                    time.sleep(min(float(query.get("timeout", "0")), 0.5))
                self.reply(status, payload)

            do_GET = do_POST = do_PATCH = do_DELETE = do_PUT = handle_any

        return Handler

    # Called with self.lock held.
    def route(self, method, path, query, body):
        state = self.state
        folders = state["folders"]

        def find(folder_id):
            for item in folders:
                if item["id"] == folder_id:
                    return item
            return None

        if method == "GET":
            simple = {
                "/rest/system/status": lambda: {"myID": state["myID"], "tilde": state["tilde"], "uptime": 5},
                "/rest/system/version": lambda: {"version": state["version"], "os": "linux"},
                "/rest/config/devices": lambda: state["devices"],
                "/rest/config/folders": lambda: folders,
                "/rest/system/connections": lambda: {"connections": state["connections"], "total": {}},
                "/rest/cluster/pending/folders": lambda: state["pendingFolders"],
                "/rest/cluster/pending/devices": lambda: state["pendingDevices"],
                "/rest/system/discovery": lambda: state["discovery"],
                "/rest/config/gui": lambda: state["gui"],
                "/rest/system/paths": lambda: state["paths"],
                "/rest/config/defaults/folder": lambda: state["defaultFolder"],
            }
            if path in simple:
                return 200, copy.deepcopy(simple[path]())
            if path.startswith("/rest/config/folders/"):
                item = find(unquote(path[len("/rest/config/folders/") :]))
                return (200, copy.deepcopy(item)) if item else (404, None)
            if path == "/rest/db/status":
                return 200, state["status"].get(
                    query.get("folder"), {"state": "idle", "needTotalItems": 0, "globalFiles": 1, "globalBytes": 10}
                )
            if path == "/rest/folder/errors":
                if find(query.get("folder")) is None:
                    return None
                return 200, {
                    "folder": query.get("folder"),
                    "errors": state["errors"].get(query.get("folder")),
                    "page": 1,
                    "perpage": 100,
                }
            if path == "/rest/svc/random/string":
                return 200, {"random": state["random"]}
            if path == "/rest/db/file":
                info = state["files"].get((query.get("folder"), query.get("file")))
                return (200, info) if info else None
            if path == "/rest/events":
                since = int(query.get("since", "0"))
                limit = int(query.get("limit", "0"))
                kinds = set(query.get("events", "").split(",")) if query.get("events") else None
                events = [e for e in state["events"] if e["id"] > since and (kinds is None or e["type"] in kinds)]
                if limit > 0:
                    events = events[-limit:]
                return 200, events
            return None
        if method == "PATCH" and path.startswith("/rest/config/folders/"):
            item = find(unquote(path[len("/rest/config/folders/") :]))
            if item is None:
                return 404, None
            item.update(json.loads(body))
            self.add_event_locked("ConfigSaved", {})
            return 200, None
        if method == "DELETE" and path.startswith("/rest/config/folders/"):
            item = find(unquote(path[len("/rest/config/folders/") :]))
            if item is None:
                return 404, None
            folders.remove(item)
            self.add_event_locked("ConfigSaved", {})
            return 200, None
        if method == "POST" and path == "/rest/config/folders":
            folders.append(json.loads(body))
            self.add_event_locked("ConfigSaved", {})
            return 200, None
        if method == "POST" and path == "/rest/db/scan":
            return 200, None
        return None

    def add_event_locked(self, kind, data):
        self.state["events"].append(
            {
                "id": self.next_event,
                "globalID": self.next_event,
                "type": kind,
                "time": "2026-11-06T00:00:00Z",
                "data": data,
            }
        )
        self.next_event += 1
