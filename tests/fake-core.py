#!/usr/bin/python3 -I
"""Test double for bin/x86_64/syncshell-core (protocol v2 over stdin/stdout,
no network). FAKE_CORE_SNAPSHOT: JSON state file, re-read for every snapshot.
FAKE_CORE_LOG: append argv and requests. FAKE_CORE_EXIT=1: exit right after
the first snapshot (a crashing core)."""
import json
import os
import sys

snapshot_path = os.environ.get("FAKE_CORE_SNAPSHOT", "")
log_path = os.environ.get("FAKE_CORE_LOG", "")


def log(entry):
    if log_path:
        with open(log_path, "a") as output:
            output.write(json.dumps(entry) + "\n")


def state():
    if snapshot_path and os.path.exists(snapshot_path):
        with open(snapshot_path) as source:
            return json.load(source)
    return {"connection": {"online": False, "phase": "discovering"}}


def emit(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


log({"argv": sys.argv[1:]})
revision = 1
emit({"v": 2, "type": "hello", "build": {"version": "fake"}})
emit({"v": 2, "type": "snapshot", "revision": revision, "state": state()})
if os.environ.get("FAKE_CORE_EXIT") == "1":
    sys.exit(3)
for line in sys.stdin:
    try:
        message = json.loads(line)
    except ValueError:
        continue
    log({"request": message})
    if message.get("type") == "shutdown":
        emit({"v": 2, "type": "end"})
        break
    data = {"folderId": "abcde-fghij"} if message.get("action") == "folder.suggest-id" else {}
    emit({"v": 2, "type": "result", "id": message.get("id"), "ok": True, "data": data})
    if message.get("type") in ("refresh", "action"):
        revision += 1
        emit({"v": 2, "type": "snapshot", "revision": revision, "state": state()})
