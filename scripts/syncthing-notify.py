#!/usr/bin/python3
"""Deliver one bounded notification from stdin without launching subprocesses."""

import json
import os
import signal
import sys

MAX_INPUT_BYTES = 32768


def read_request(stream):
    raw = stream.read(MAX_INPUT_BYTES + 1)
    if len(raw) > MAX_INPUT_BYTES:
        raise ValueError("input limit")
    request = json.loads(raw.decode("utf-8"))
    if not isinstance(request, dict) or set(request) != {"summary", "body", "expireTimeout"}:
        raise ValueError("request fields")
    for key in ("summary", "body"):
        text = request[key]
        if not isinstance(text, str) or "\x00" in text:
            raise ValueError("notification text")
        text.encode("utf-8")
    timeout = request["expireTimeout"]
    if type(timeout) is not int or not -1 <= timeout <= 2147483647:
        raise ValueError("expiration")
    return request


def deliver(request):
    import dbus

    address = os.environ.get("DBUS_SESSION_BUS_ADDRESS")
    if not address:
        runtime = os.environ.get("XDG_RUNTIME_DIR", "")
        if not runtime.startswith("/"):
            raise RuntimeError("session bus unavailable")
        address = "unix:path=" + runtime + "/bus"
    # Connect to the existing bus directly; never autolaunch a bus subprocess.
    bus = dbus.bus.BusConnection(address)
    try:
        notification_id = bus.call_blocking(
            "org.freedesktop.Notifications", "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications", "Notify", "susssasa{sv}i",
            ("omarchy-action", 0, "", request["summary"], request["body"], [],
             {"urgency": dbus.Byte(0)}, request["expireTimeout"]),
            timeout=2.0,
        )
        if not isinstance(notification_id, int) or not 0 < notification_id <= 4294967295:
            raise RuntimeError("invalid notification reply")
    finally:
        bus.close()


def deadline(_signal, _frame):
    raise TimeoutError("notification deadline")


def main():
    if len(sys.argv) != 1:
        print("Notification helper accepts stdin only.", file=sys.stderr)
        return 2
    # Bound stdin and Python work. The caller also enforces a hard deadline
    # because native D-Bus connection setup may defer Python signal handling.
    signal.signal(signal.SIGALRM, deadline)
    signal.alarm(3)
    try:
        request = read_request(sys.stdin.buffer)
        deliver(request)
    except Exception:
        # D-Bus errors can contain request data. Keep diagnostics constant.
        print("Desktop notification delivery failed.", file=sys.stderr)
        return 1
    finally:
        signal.alarm(0)
    return 0


if __name__ == "__main__":
    sys.exit(main())
