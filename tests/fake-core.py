#!/usr/bin/python3 -I
"""Test double for bin/<arch>/foamy-syncthing-helper's `stream` (protocol v2
over stdin/stdout, no network). FAKE_CORE_SNAPSHOT: JSON state file, re-read
for every snapshot. FAKE_CORE_LOG: append argv and requests. FAKE_CORE_EXIT=1:
exit right after the first snapshot (a crashing core). `conflicts` and
`pick-folder` run the real helper at FAKE_CORE_HELPER (same process: exec);
FAKE_PICKER_BUS is then pick-folder's session bus."""
import json
import os
import sys

if sys.argv[1:2] == ["conflicts"] or sys.argv[1:2] == ["pick-folder"]:
    if sys.argv[1:2] == ["pick-folder"] and os.environ.get("FAKE_PICKER_BUS"):
        os.environ["DBUS_SESSION_BUS_ADDRESS"] = os.environ["FAKE_PICKER_BUS"]
    helper = os.environ["FAKE_CORE_HELPER"]
    os.execv(helper, [helper] + sys.argv[1:])

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
