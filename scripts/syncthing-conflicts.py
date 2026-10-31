#!/usr/bin/python3
"""Report which configured roots (stdin JSON list) contain Syncthing conflict
files, as a JSON list of booleans, without exposing paths in arguments."""

import fnmatch
import json
import os
import signal
import stat
import sys

MAX_INPUT_BYTES = 1048576


def read_paths(stream):
    raw = stream.read(MAX_INPUT_BYTES + 1)
    if len(raw) > MAX_INPUT_BYTES:
        raise ValueError("input limit")
    paths = json.loads(raw.decode("utf-8"))
    if not isinstance(paths, list):
        raise ValueError("path list")
    for path in paths:
        if not isinstance(path, str) or not path or "\x00" in path:
            raise ValueError("path")
        path.encode("utf-8")
    return paths


# Syncthing's own bookkeeping, not user data: versioned copies of resolved
# conflicts keep the .sync-conflict- stem, so they would match forever.
SKIPPED_DIRECTORIES = frozenset((".stversions", ".stfolder"))


def folder_has_conflict(folder):
    try:
        mode = os.stat(folder).st_mode
    except (FileNotFoundError, NotADirectoryError):
        return False
    if not stat.S_ISDIR(mode):
        return False
    # Follow configured root links, but no descendant links, like find -H.
    # Stream depth first so wide directories do not require an entry list.
    stack = [os.scandir(folder)]
    try:
        while stack:
            try:
                entry = next(stack[-1])
            except StopIteration:
                stack.pop().close()
                continue
            try:
                if entry.is_dir(follow_symlinks=False):
                    if entry.name not in SKIPPED_DIRECTORIES:
                        stack.append(os.scandir(entry.path))
                elif (fnmatch.fnmatchcase(entry.name, "*.sync-conflict-*")
                      and entry.is_file(follow_symlinks=False)):
                    return True
            except FileNotFoundError:
                # A concurrent sync can remove an entry while it is scanned.
                continue
    finally:
        for entries in stack:
            entries.close()
    return False


def conflict_flags(paths):
    """One flag per configured root, in input order."""
    return [folder_has_conflict(folder) for folder in paths]


def has_conflicts(paths):
    return any(folder_has_conflict(folder) for folder in paths)


def deadline(_signal, _frame):
    raise TimeoutError("scan deadline")


def main():
    if len(sys.argv) != 1:
        print("Conflict helper accepts stdin only.", file=sys.stderr)
        return 2
    # The QML caller also kills a worker stuck in a filesystem syscall at 30 s.
    signal.signal(signal.SIGALRM, deadline)
    signal.alarm(25)
    try:
        flags = conflict_flags(read_paths(sys.stdin.buffer))
    except Exception:
        # Filesystem exceptions contain private paths; keep diagnostics constant.
        print("Could not check for Syncthing conflicts.", file=sys.stderr)
        return 1
    finally:
        signal.alarm(0)
    # Index-aligned booleans only: no paths leave the helper.
    print(json.dumps(flags))
    return 0


if __name__ == "__main__":
    sys.exit(main())
