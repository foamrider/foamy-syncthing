#!/usr/bin/python3 -I
"""Test double for bin/<arch>/foamy-syncthing-helper's `stream` (protocol v2
over stdin/stdout, no network). FAKE_CORE_SNAPSHOT: JSON state file, re-read
for every snapshot. FAKE_CORE_LOG: append argv and requests. FAKE_CORE_EXIT=1:
exit right after the first snapshot (a crashing core). Like the real core, a
refresh or action publishes its new snapshot before its result.

Requests the panel never sends, for the runtime scenes: action `fake.hold`
gets no result; `fake.emit` writes args.line to stdout as it is (a broken
core); `fake.exit` exits with status 3 (a core that dies). FAKE_CORE_FAIL: a
JSON object; the key "<action> <folderId>" (or "<action>") makes that action
fail with the value as its error and no new snapshot, or, with the value
"hold", get no result at all.

`conflicts` and `pick-folder` run the real helper at FAKE_CORE_HELPER (same
process: exec); FAKE_PICKER_BUS is then pick-folder's session bus. With
FAKE_PICKER_PATH, `pick-folder` answers that path as the chosen directory
instead (no bus, no portal)."""

import json
import os
import sys

if sys.argv[1:2] == ["pick-folder"] and os.environ.get("FAKE_PICKER_PATH"):
    print(json.dumps({"ok": True, "path": os.environ["FAKE_PICKER_PATH"]}))
    sys.exit(0)
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
failures = json.loads(os.environ.get("FAKE_CORE_FAIL") or "{}")
for line in sys.stdin:
    try:
        message = json.loads(line)
    except ValueError:
        continue
    log({"request": message})
    if message.get("type") == "shutdown":
        emit({"v": 2, "type": "end"})
        break
    action = message.get("action", "")
    args = message.get("args") or {}
    if action == "fake.hold":
        continue
    if action == "fake.emit":
        sys.stdout.write(args["line"] + "\n")
        sys.stdout.flush()
        continue
    if action == "fake.exit":
        sys.exit(3)
    failure = failures.get(f"{action} {args.get('folderId', '')}".strip(), failures.get(action))
    if failure == "hold":
        continue
    if failure is not None:
        emit({"v": 2, "type": "result", "id": message.get("id"), "ok": False, "error": failure})
        continue
    if message.get("type") in ("refresh", "action"):
        revision += 1
        emit({"v": 2, "type": "snapshot", "revision": revision, "state": state()})
    data = {"folderId": "abcde-fghij"} if action == "folder.suggest-id" else {}
    emit({"v": 2, "type": "result", "id": message.get("id"), "ok": True, "data": data})
