"""The native core (`stream`, protocol v2) at its process boundary: discovery,
the snapshot, requests, folder and service actions, TLS pinning, events and
the protocol bounds, against a fake Syncthing and a fake systemctl
(tests/core_harness.py). Nothing here reaches a real Syncthing or the user
service."""

import json
import sys
import unittest
from pathlib import Path
from urllib.parse import parse_qs, quote, urlsplit

sys.path.insert(0, str(Path(__file__).resolve().parent))

import fake_syncthing  # noqa: E402
from core_harness import Core, PLUGIN_FLAGS, UNIT, silent_dns_prefix  # noqa: E402
from fake_syncthing import API_KEY, LOCAL_ID, OTHER_ID, PEER_ID  # noqa: E402

SHOW = [
    "systemctl",
    "--user",
    "show",
    UNIT,
    "--no-pager",
    "--property=LoadState",
    "--property=ActiveState",
    "--property=UnitFileState",
    "--property=FragmentPath",
    "--property=ExecStart",
    "--property=Environment",
]


class CoreTest(unittest.TestCase):
    def core(self, **options):
        core = Core(**options)
        self.addCleanup(core.close)
        return core

    def folder_core(self, paused=False, **options):
        core = self.core(start=False, **options)
        docs = core.home / "Docs"
        docs.mkdir()
        core.syncthing.state["folders"] = [fake_syncthing.folder(docs, paused=paused)]
        core.syncthing.state["status"]["docs"] = {
            "state": "idle",
            "needTotalItems": 2,
            "needBytes": 300,
            "globalFiles": 7,
            "globalBytes": 4000,
            "pullErrors": 0,
        }
        core.start()
        return core, docs

    def assert_secret_free(self, core):
        self.assertNotIn(API_KEY.encode(), getattr(core, "raw", b""))
        self.assertNotIn(API_KEY.encode(), core.stderr)


class SnapshotTests(CoreTest):
    def test_hello_then_a_complete_snapshot(self):
        core, docs = self.folder_core()
        hello = core.next_frame()
        self.assertEqual(hello["v"], 2)
        self.assertEqual(hello["type"], "hello")
        self.assertIsInstance(hello["build"]["version"], str)
        snapshot = core.snapshot(lambda s: s["connection"]["phase"] == "ready")
        self.assertIsInstance(snapshot["revision"], int)
        self.assertGreaterEqual(snapshot["revision"], 1)
        state = snapshot["state"]
        self.assertEqual(
            state["connection"],
            {
                "phase": "ready",
                "endpoint": "http://" + core.syncthing.address,
                "healthy": True,
                "authorized": True,
                "online": True,
                "fresh": True,
            },
        )
        self.assertEqual(state["identity"], {"deviceId": LOCAL_ID, "version": "v2.0.10"})
        # Sorted by ID, names as configured, connection state from /rest/system/connections.
        self.assertEqual(
            state["devices"],
            [
                {"id": LOCAL_ID, "name": "this", "untrusted": False, "connected": False},
                {"id": PEER_ID, "name": "laptop", "untrusted": False, "connected": True, "version": "v2.0.9"},
                {"id": OTHER_ID, "name": "<phone & co>", "untrusted": True, "connected": False},
            ],
        )
        self.assertEqual(
            state["folders"],
            [
                {
                    "id": "docs",
                    "label": "Documents",
                    "path": str(docs),
                    "paused": False,
                    "markerName": ".stfolder",
                    "devices": [{"id": LOCAL_ID}, {"id": PEER_ID}],
                    "status": {
                        "state": "idle",
                        "pullErrors": 0,
                        "needTotalItems": 2,
                        "needBytes": 300,
                        "globalFiles": 7,
                        "globalBytes": 4000,
                        "errors": None,
                    },
                }
            ],
        )
        self.assertEqual(state["pendingFolders"], {})
        self.assertEqual(
            state["counts"],
            {"folders": 1, "devices": 2, "connectedDevices": 1, "folderProblems": 0, "syncingFolders": 1},
        )
        self.assertEqual(state["truncation"], {})
        self.assertEqual(
            state["webUi"],
            {"url": "http://" + core.syncthing.address, "theme": "default", "guiAssets": "/usr/share/syncthing/gui"},
        )
        self.assertEqual(state["installation"], {"executablePath": str(core.bin / "syncthing"), "available": True})
        self.assertEqual(state["activity"], {"files": None})
        self.assertEqual(
            state["lifecycle"],
            {
                "kind": "systemd-user",
                "unit": UNIT,
                "classification": "managed",
                "available": True,
                "active": True,
                "activeState": "active",
                "unitFileState": "enabled",
                "authorized": True,
                "targetMatch": True,
                "canControl": True,
                "canStart": False,
            },
        )
        self.assertEqual(
            sorted(state),
            sorted(
                [
                    "connection",
                    "identity",
                    "devices",
                    "folders",
                    "pendingFolders",
                    "pendingDevices",
                    "nearbyDevices",
                    "activity",
                    "webUi",
                    "installation",
                    "counts",
                    "truncation",
                    "lifecycle",
                ]
            ),
        )
        core.request("shutdown", "bye")
        self.assertEqual(core.finish(), 0)
        self.assert_secret_free(core)

    def test_the_api_key_goes_only_to_authenticated_endpoints(self):
        core, _ = self.folder_core()
        core.ready()
        core.request("refresh", "1")
        health = core.syncthing.calls(prefix="/rest/noauth/health")
        self.assertTrue(health)
        self.assertTrue(all(call["key"] is None for call in health))
        others = [call for call in core.syncthing.calls() if call["path"] != "/rest/noauth/health"]
        self.assertTrue(others)
        self.assertTrue(all(call["key"] == API_KEY for call in others), others[:3])
        cmdline = Path(f"/proc/{core.process.pid}/cmdline").read_bytes()
        self.assertNotIn(API_KEY.encode(), cmdline)
        environ = Path(f"/proc/{core.process.pid}/environ").read_bytes()
        self.assertNotIn(API_KEY.encode(), environ)
        core.request("shutdown", "2")
        core.finish()
        self.assert_secret_free(core)

    def test_bounds_sorting_and_problem_counts(self):
        core = self.core(start=False)
        state = core.syncthing.state
        state["folders"] = [
            fake_syncthing.folder(core.home / f"f{i}", folder_id=f"f{i:03d}", label=" padded ") for i in range(130)
        ]
        state["errors"]["f000"] = [{"path": f"z{i}", "error": "denied"} for i in range(6)] + [
            {"path": "a", "error": "x" * 2000}
        ]
        state["status"]["f001"] = {"state": "error", "error": "  folder marker missing  "}
        state["pendingFolders"] = {
            "offer": {
                "offeredBy": {
                    PEER_ID: {"label": "Shared", "receiveEncrypted": False, "remoteEncrypted": True, "time": "x"}
                }
            }
        }
        state["pendingDevices"] = {"DDDDDDD": {"name": "new", "address": "192.0.2.1:22000", "time": "x"}}
        state["discovery"] = {
            "DDDDDDD": {"addresses": [f"tcp://192.0.2.{i}:22000" for i in range(10)]},
            PEER_ID: {"addresses": ["tcp://x"]},
        }
        core.start()
        snapshot = core.ready()["state"]
        self.assertEqual(len(snapshot["folders"]), 128)
        self.assertEqual(snapshot["folders"][0]["id"], "f000")
        self.assertEqual(snapshot["folders"][0]["label"], "padded")
        errors = snapshot["folders"][0]["status"]["errors"]
        self.assertEqual([e["path"] for e in errors], ["a", "z0", "z1", "z2"])
        self.assertEqual(len(errors[0]["error"]), 1024)
        self.assertEqual(snapshot["folders"][1]["status"]["error"], "folder marker missing")
        self.assertEqual(snapshot["truncation"], {"folders": 2, "folderErrors": 3})
        self.assertEqual(snapshot["counts"]["folderProblems"], 2)
        self.assertEqual(
            snapshot["pendingFolders"],
            {
                "offer": {
                    "offeredBy": {PEER_ID: {"label": "Shared", "receiveEncrypted": False, "remoteEncrypted": True}}
                }
            },
        )
        self.assertEqual(snapshot["pendingDevices"], [{"id": "DDDDDDD", "name": "new", "address": "192.0.2.1:22000"}])
        self.assertEqual(
            snapshot["nearbyDevices"], [{"id": "DDDDDDD", "addresses": [f"tcp://192.0.2.{i}:22000" for i in range(8)]}]
        )
        core.request("shutdown", "1")
        core.finish()


class RequestTests(CoreTest):
    def test_refresh_publishes_changes_before_its_result(self):
        core, _ = self.folder_core()
        first = core.ready()
        # Idle Syncthing: the event loop publishes nothing, so every frame
        # after a request belongs to that request.
        before = len(core.frames)
        result = core.request("refresh", "1")
        self.assertEqual(result, {"v": 2, "type": "result", "id": "1", "ok": True})
        self.assertEqual([f["type"] for f in core.frames[before:]], ["result"], "an unchanged refresh sent a snapshot")
        with core.syncthing.lock:
            core.syncthing.state["devices"][0]["name"] = "renamed"
        before = len(core.frames)
        result = core.request("refresh", "2")
        self.assertTrue(result["ok"], result)
        frames = core.frames[before:]
        self.assertEqual([f["type"] for f in frames], ["snapshot", "result"], "the change must come before the result")
        self.assertIn("renamed", [d["name"] for d in frames[0]["state"]["devices"]])
        self.assertGreater(frames[0]["revision"], first["revision"])
        core.request("shutdown", "3")
        self.assertEqual(core.finish(), 0)
        self.assertEqual(core.frames[-1], {"v": 2, "type": "end", "reason": "shutdown"})

    def test_configure_validates_the_intervals(self):
        core, _ = self.folder_core()
        core.ready()
        result = core.request("configure", "1", config={"probeIntervalSeconds": 5, "refreshIntervalSeconds": 60})
        self.assertTrue(result["ok"], result)
        self.assertNotIn("desiredState", core.latest_state()["lifecycle"])
        for config, message in [
            ({"probeIntervalSeconds": 0}, "probe interval must be between 1 and 3600 seconds"),
            ({"refreshIntervalSeconds": 59}, "refresh interval must be between 60 and 3600 seconds"),
            # The removed serviceState setting is no longer part of the request.
            ({"desiredServiceState": "enabled"}, "configuration request is invalid"),
            ({"unknown": 1}, "configuration request is invalid"),
            ("x", "configuration request is invalid"),
        ]:
            result = core.request("configure", "c" + json.dumps(config), config=config)
            self.assertEqual(result["ok"], False)
            self.assertEqual(result["error"], {"code": "invalid_config", "message": message})
        core.request("shutdown", "2")
        core.finish()

    def test_stdin_end_finishes_the_stream(self):
        core, _ = self.folder_core()
        core.ready()
        self.assertEqual(core.finish(), 0)
        self.assertEqual(core.frames[-1], {"v": 2, "type": "end", "reason": "stdin"})

    def test_protocol_violations_are_fatal(self):
        # One line per check; protocol.rs `requests_are_strict` has the rest
        # of the parser's rows.
        cases = [
            (b"not json", "protocol_request", "request is malformed"),
            (b'{"v":1,"type":"refresh","id":"1"}', "protocol_request", "protocol major 2 required"),
            (b'{"v":2,"type":"refresh","id":"' + b"x" * 129 + b'"}', "protocol_request", "request id is invalid"),
            (b'{"v":2,"type":"reboot","id":"1"}', "protocol_request", "request type is unsupported"),
        ]
        for line, code, message in cases:
            with self.subTest(line=line[:40]):
                core, _ = self.folder_core()
                core.ready()
                core.send(line)
                fatal = core.wait_for(lambda f: f["type"] == "fatal")
                self.assertEqual(fatal, {"v": 2, "type": "fatal", "code": code, "message": message})
                self.assertEqual(core.process.wait(timeout=10), 1)
                core.close()

    def test_overlong_input_line_is_fatal(self):
        core, _ = self.folder_core()
        core.ready()
        try:
            core.process.stdin.write(b"x" * (8 << 20))
            core.process.stdin.flush()
        except BrokenPipeError:
            pass
        fatal = core.wait_for(lambda f: f["type"] == "fatal", timeout=20)
        self.assertEqual(
            fatal, {"v": 2, "type": "fatal", "code": "line_too_large", "message": "protocol input is invalid"}
        )
        self.assertEqual(core.process.wait(timeout=10), 1)

    def test_action_arguments_are_strict(self):
        core, _ = self.folder_core()
        core.ready()
        # A field of another action, and arguments that are not an object.
        # actions.rs `argument_shapes_are_per_action` has the per-action rows.
        for request_id, args in [("1", {"folderId": "docs", "path": "/x"}), ("3", [])]:
            result = core.action("folder.pause", request_id, args)
            self.assertEqual(
                result["error"], {"code": "invalid_action", "message": "action arguments are invalid"}, request_id
            )
        self.assertEqual(
            core.action("does.not-exist", "6")["error"],
            {"code": "unsupported_action", "message": "action is not supported"},
        )
        self.assertEqual(core.request("action", "7", action="folder.suggest-id")["error"]["code"], "invalid_action")
        self.assertFalse(core.syncthing.calls("PATCH"))
        core.request("shutdown", "8")
        core.finish()


class FolderActionTests(CoreTest):
    def test_pause_resume_and_forget(self):
        core, _ = self.folder_core()
        core.ready()
        result = core.action("folder.pause", "1", {"folderId": "docs"})
        self.assertTrue(result["ok"], result)
        patch = core.syncthing.calls("PATCH")
        self.assertEqual(
            [(c["target"], json.loads(c["body"])) for c in patch], [("/rest/config/folders/docs", {"paused": True})]
        )
        self.assertEqual(patch[0]["headers"].get("Content-Type"), "application/json")
        self.assertTrue(core.latest_state()["folders"][0]["paused"])
        self.assertEqual(
            core.action("folder.pause", "2", {"folderId": "docs"})["error"],
            {"code": "folder_paused", "message": "folder is already paused"},
        )
        result = core.action("folder.forget", "3", {"folderId": "docs"})
        self.assertTrue(result["ok"], result)
        self.assertEqual([c["target"] for c in core.syncthing.calls("DELETE")], ["/rest/config/folders/docs"])
        self.assertEqual(core.latest_state()["folders"], None)
        self.assertEqual(
            core.action("folder.resume", "4", {"folderId": "docs"})["error"],
            {"code": "folder_missing", "message": "folder is no longer configured"},
        )
        core.request("shutdown", "5")
        core.finish()

    def test_forget_requires_a_paused_folder(self):
        core, _ = self.folder_core()
        core.ready()
        self.assertEqual(
            core.action("folder.forget", "1", {"folderId": "docs"})["error"],
            {"code": "folder_active", "message": "folder must be paused before it is forgotten"},
        )
        self.assertEqual(
            core.action("folder.resume", "2", {"folderId": "docs"})["error"],
            {"code": "folder_active", "message": "folder is already active"},
        )
        self.assertFalse(core.syncthing.calls("DELETE"))
        # A folder whose path changed behind the snapshot is refused.
        core.syncthing.state["folders"][0]["path"] = "/elsewhere"
        self.assertEqual(
            core.action("folder.pause", "3", {"folderId": "docs"})["error"],
            {"code": "folder_changed", "message": "folder changed before the action"},
        )
        self.assertFalse(core.syncthing.calls("PATCH"))
        core.request("shutdown", "4")
        core.finish()

    def test_suggest_id_and_recheck_errors(self):
        core, _ = self.folder_core()
        core.syncthing.state["status"]["docs"]["pullErrors"] = 1
        core.ready()
        core.request("refresh", "0")
        result = core.action("folder.suggest-id", "1")
        self.assertEqual(result["data"], {"folderId": "abcdefghij"})
        self.assertEqual(
            core.syncthing.calls(prefix="/rest/svc/random")[-1]["target"], "/rest/svc/random/string?length=10"
        )
        core.syncthing.state["random"] = "short"
        self.assertEqual(
            core.action("folder.suggest-id", "2")["error"],
            {"code": "suggestion_invalid", "message": "Syncthing returned an invalid folder ID"},
        )
        result = core.action("folder.recheck-errors", "3")
        self.assertTrue(result["ok"], result)
        self.assertEqual([c["target"] for c in core.syncthing.calls("POST")], ["/rest/db/scan?folder=docs"])
        core.request("shutdown", "4")
        core.finish()

    def test_add_existing_directory(self):
        core, _ = self.folder_core()
        core.ready()
        target = core.home / "Music"
        target.mkdir()
        (core.home / "link").symlink_to(target)
        args = {
            "folderId": "music-1",
            "path": str(core.home / "link") + "/",
            "createDirectory": False,
            "label": "",
            "deviceIds": [PEER_ID],
            "pendingDeviceId": "",
        }
        result = core.action("folder.add-existing", "1", args)
        self.assertEqual(result, {"v": 2, "type": "result", "id": "1", "ok": True, "data": {"folderId": "music-1"}})
        posted = json.loads(core.syncthing.calls("POST", "/rest/config/folders")[-1]["body"])
        self.assertEqual(posted["id"], "music-1")
        self.assertEqual(posted["label"], "Music")
        self.assertEqual(posted["path"], str(target))
        self.assertEqual(posted["paused"], False)
        self.assertEqual(posted["devices"], [{"deviceID": LOCAL_ID}, {"deviceID": PEER_ID}])
        self.assertEqual(posted["rescanIntervalS"], 3600)
        self.assertEqual(posted["minDiskFree"], {"value": 1, "unit": "%"})
        self.assertEqual([f["id"] for f in core.latest_state()["folders"]], ["docs", "music-1"])
        core.request("shutdown", "2")
        core.finish()

    def test_add_existing_rejections(self):
        core, docs = self.folder_core()
        core.ready()
        (core.home / "file").write_text("x")
        base = {"folderId": "new", "createDirectory": False, "label": "L", "deviceIds": [], "pendingDeviceId": ""}
        cases = [
            ({"path": "relative"}, "path_invalid", "folder path must be absolute"),
            ({"path": str(core.home / "missing" / "deeper")}, "path_missing", "folder path does not exist"),
            ({"path": str(core.home / "file")}, "path_invalid", "folder path is not a directory"),
            ({"path": str(docs / "inner")}, "path_missing", "folder path does not exist"),
            ({"path": str(core.home)}, "path_overlap", "folder path overlaps an existing folder"),
            ({"path": str(docs)}, "path_overlap", "folder path overlaps an existing folder"),
            ({"path": str(core.home / "bin"), "folderId": "docs"}, "folder_exists", "folder ID is already configured"),
            ({"path": str(core.home / "bin"), "folderId": "bad id"}, "folder_id_invalid", "folder ID is invalid"),
            ({"path": str(core.home / "bin"), "folderId": "x" * 129}, "folder_id_invalid", "folder ID is invalid"),
            (
                {"path": str(core.home / "bin"), "deviceIds": [OTHER_ID]},
                "device_invalid",
                "selected remote device is unavailable or untrusted",
            ),
            (
                {"path": str(core.home / "bin"), "deviceIds": [LOCAL_ID]},
                "device_invalid",
                "selected remote device is unavailable or untrusted",
            ),
            (
                {"path": str(core.home / "bin"), "pendingDeviceId": PEER_ID},
                "offer_missing",
                "selected folder offer is no longer available",
            ),
        ]
        for index, (override, code, message) in enumerate(cases):
            result = core.action("folder.add-existing", str(index), dict(base, **override))
            self.assertEqual(result.get("error"), {"code": code, "message": message}, override)
        self.assertFalse(core.syncthing.calls("POST", "/rest/config/folders"))
        core.request("shutdown", "x")
        core.finish()

    def test_accept_a_pending_offer(self):
        core = self.core(start=False)

        def offer(label, encrypted=False):
            return {"offeredBy": {PEER_ID: {"label": label, "receiveEncrypted": False, "remoteEncrypted": encrypted}}}

        core.syncthing.state["pendingFolders"] = {
            "offer-1": offer("Shared photos"),
            "enc-1": offer("Secret", encrypted=True),
            "offer-3": offer("Third"),
        }
        for name in ("Photos", "Other", "Enc", "Third"):
            (core.home / name).mkdir()
        core.start()
        core.ready()
        args = {
            "folderId": "offer-1",
            "path": str(core.home / "Photos"),
            "createDirectory": False,
            "label": "",
            "deviceIds": [PEER_ID],
            "pendingDeviceId": PEER_ID,
        }
        result = core.action("folder.add-existing", "1", args)
        self.assertTrue(result["ok"], result)
        posted = json.loads(core.syncthing.calls("POST", "/rest/config/folders")[-1]["body"])
        self.assertEqual(posted["label"], "Shared photos")
        self.assertEqual(posted["devices"], [{"deviceID": LOCAL_ID}, {"deviceID": PEER_ID}])
        # An encrypted share must never be accepted as a plain one, and an
        # accepted offer keeps the device that offered it.
        for request_id, override, error in [
            (
                "2",
                {"folderId": "offer-2", "path": str(core.home / "Other")},
                {"code": "offer_missing", "message": "selected folder offer is no longer available"},
            ),
            (
                "3",
                {"folderId": "enc-1", "path": str(core.home / "Enc")},
                {"code": "offer_encrypted", "message": "encrypted folder offers are unsupported"},
            ),
            (
                "4",
                {"folderId": "offer-3", "path": str(core.home / "Third"), "deviceIds": []},
                {"code": "offer_device_missing", "message": "offering device must remain selected"},
            ),
        ]:
            result = core.action("folder.add-existing", request_id, dict(args, **override))
            self.assertEqual(result.get("error"), error, override)
        self.assertEqual(len(core.syncthing.calls("POST", "/rest/config/folders")), 1)
        core.request("shutdown", "5")
        core.finish()

    def test_actions_need_syncthing_online(self):
        core = self.core(start=False)
        core.syncthing.close()
        core.start()
        state = core.ready()["state"]
        self.assertEqual(state["connection"]["phase"], "error")
        self.assertEqual(
            state["connection"]["error"], {"code": "connection", "message": "could not connect to Syncthing"}
        )
        for action, args in [
            ("folder.pause", {"folderId": "docs"}),
            ("folder.suggest-id", {}),
            ("folder.recheck-errors", {}),
            ("folder.add-existing", {"folderId": "x", "path": "/tmp"}),
        ]:
            self.assertEqual(
                core.action(action, action, args)["error"], {"code": "offline", "message": "Syncthing is not online"}
            )
        result = core.request("refresh", "r")
        self.assertEqual(result["error"], {"code": "connection", "message": "could not connect to Syncthing"})
        core.request("shutdown", "s")
        core.finish()


class LifecycleTests(CoreTest):
    def test_stop_and_start_run_the_exact_unit(self):
        core, _ = self.folder_core()
        core.ready()
        # Syncthing stops with its service.
        core.syncthing.down = lambda: core.systemctl_state()["ActiveState"] != "active"
        result = core.action("lifecycle.stop", "1")
        self.assertTrue(result["ok"], result)
        self.assertEqual(core.systemctl_state()["ActiveState"], "inactive")
        lifecycle = core.latest_state()["lifecycle"]
        self.assertEqual((lifecycle["active"], lifecycle["canControl"]), (False, True))
        self.assertEqual(lifecycle["classification"], "stopped-candidate")
        self.assertTrue(lifecycle["canStart"])
        self.assertFalse(core.latest_state()["connection"]["online"])
        result = core.action("lifecycle.start", "3")
        self.assertTrue(result["ok"], result)
        self.assertTrue(core.latest_state()["connection"]["online"])
        self.assertEqual(core.latest_state()["lifecycle"]["classification"], "managed")
        # Already in the requested state: nothing runs.
        self.assertTrue(core.action("lifecycle.start", "2")["ok"])
        calls = [c for c in core.calls() if c[0] == "systemctl"]
        actions = [c for c in calls if c != SHOW]
        self.assertEqual(actions, [["systemctl", "--user", "stop", UNIT], ["systemctl", "--user", "start", UNIT]])
        self.assertTrue(all(c == SHOW for c in calls if c not in actions))
        core.request("shutdown", "4")
        core.finish()

    def test_lifecycle_failures_are_sanitized(self):
        core, _ = self.folder_core(systemctl={"fail_action": True})
        core.ready()
        result = core.action("lifecycle.stop", "1")
        self.assertEqual(
            result["error"], {"code": "lifecycle_failed", "message": "authorized user-service action failed"}
        )
        core.set_systemctl({"fail": True})
        core.request("refresh", "2")
        lifecycle = core.latest_state()["lifecycle"]
        self.assertEqual(lifecycle["error"], "could not inspect authorized user service")
        self.assertFalse(lifecycle["canControl"])
        core.request("shutdown", "3")
        core.finish()
        self.assertNotIn(b"Canary", core.raw + core.stderr)

    def test_a_unit_for_another_config_is_external(self):
        core, _ = self.folder_core(
            systemctl={"ExecStart": "{ path=/usr/bin/syncthing ; argv[]=/usr/bin/syncthing serve --home=/srv/other ; }"}
        )
        lifecycle = core.ready()["state"]["lifecycle"]
        self.assertEqual(
            (lifecycle["targetMatch"], lifecycle["classification"], lifecycle["canControl"]), (False, "external", False)
        )
        self.assertEqual(
            core.action("lifecycle.stop", "1")["error"],
            {"code": "lifecycle_forbidden", "message": "lifecycle binding does not apply to this target"},
        )
        # The unit's environment or a -H/-C/--config option names the directory.
        for overrides, matches in [
            ({"Environment": "STHOMEDIR=" + str(core.config_dir)}, True),
            ({"ExecStart": "/usr/bin/syncthing serve -H /srv/x"}, False),
            ({"ExecStart": "/usr/bin/syncthing serve -C" + str(core.config_dir)}, True),
            ({"ExecStart": '"/usr/bin/syncthing" "--config" "/srv/x;"'}, False),
            ({"LoadState": "not-found"}, False),
            ({"FragmentPath": ""}, False),
        ]:
            core.set_systemctl(overrides)
            core.request("refresh", json.dumps(overrides))
            self.assertEqual(core.latest_state()["lifecycle"]["targetMatch"], matches, overrides)
        self.assertFalse([c for c in core.calls() if c[0] == "systemctl" and c != SHOW])
        core.request("shutdown", "4")
        core.finish()

    def test_without_authority_nothing_runs(self):
        core, _ = self.folder_core(flags=["--probe-interval-seconds", "15"])
        lifecycle = core.ready()["state"]["lifecycle"]
        self.assertEqual(
            lifecycle,
            {
                "kind": "systemd-user",
                "classification": "external",
                "available": False,
                "active": False,
                "authorized": False,
                "targetMatch": False,
                "canControl": False,
                "canStart": False,
            },
        )
        self.assertEqual(core.action("lifecycle.start", "1")["error"]["code"], "lifecycle_forbidden")
        self.assertFalse([c for c in core.calls() if c[0] == "systemctl"])
        core.request("shutdown", "2")
        core.finish()


class ArgumentTests(CoreTest):
    def run_core(self, flags, config=True):
        core = self.core(flags=flags, config=config, start=False)
        core.start()
        code = core.finish()
        return core, code

    def test_invalid_invocations_exit_before_hello(self):
        # One row per kind of refusal; mod.rs `go_flag_rules_and_syncshell_checks`
        # has the parser's other rows. The reason is one line on stderr.
        for flags, reason in (
            (["--unknown"], "invalid command arguments"),
            (["--probe-interval-seconds", "3601"], "probe interval must be between 1 and 3600 seconds"),
            (["--lifecycle-authorized"], "authorized lifecycle binding is incomplete"),
            (
                ["--lifecycle-authorized", "--lifecycle-kind", "systemd-user", "--lifecycle-unit", "a\nb.service"],
                "lifecycle unit is invalid",
            ),
        ):
            with self.subTest(flags=flags):
                core, code = self.run_core(flags)
                self.assertEqual(code, 1)
                self.assertEqual(core.frames, [])
                self.assertEqual(core.stderr, f"foamy-syncthing-helper: {reason}\n".encode())

    def test_missing_configuration_exits_before_hello(self):
        core, code = self.run_core(PLUGIN_FLAGS, config=False)
        self.assertEqual(code, 1)
        self.assertEqual(core.frames, [])
        self.assertIn(b"Syncthing configuration was not found", core.stderr)

    def test_explicit_config_and_expected_identity(self):
        core = self.core(start=False)
        other = core.home / "elsewhere" / "config.xml"
        other.parent.mkdir()
        core.config_path.rename(other)
        core.flags = ["--config", str(other), "--expected-device-id", " " + LOCAL_ID + " "]
        core.start()
        state = core.ready()["state"]
        self.assertTrue(state["connection"]["online"])
        core.request("shutdown", "1")
        core.finish()
        core = self.core(flags=["--expected-device-id", PEER_ID] + PLUGIN_FLAGS)
        state = core.ready()["state"]
        self.assertEqual(
            state["connection"]["error"],
            {"code": "identity", "message": "Syncthing device identity does not match the selected target"},
        )
        self.assertFalse(state["connection"]["online"])
        self.assertEqual((state["lifecycle"]["classification"], state["lifecycle"]["canControl"]), ("external", False))
        core.request("shutdown", "2")
        core.finish()

    def test_configuration_discovery(self):
        # Two candidates: `syncthing paths` decides.
        core = self.core(start=False)
        second = core.home / "config" / "syncthing" / "config.xml"
        second.parent.mkdir(parents=True)
        second.write_text("<configuration/>")
        core.env["FAKE_SYNCTHING_CONFIG"] = str(core.config_path)
        core.start()
        self.assertTrue(core.ready()["state"]["connection"]["online"])
        self.assertIn(["syncthing", "paths"], core.calls())
        core.request("shutdown", "1")
        core.finish()
        # Ambiguous without an answer from `syncthing paths`.
        core = self.core(start=False)
        second = core.home / "config" / "syncthing" / "config.xml"
        second.parent.mkdir(parents=True)
        second.write_text("<configuration/>")
        core.start()
        self.assertEqual(core.finish(), 1)
        self.assertIn(b"multiple Syncthing configurations were found", core.stderr)
        # STHOMEDIR is a candidate too.
        core = self.core(start=False)
        moved = core.home / "sthome"
        moved.mkdir()
        core.config_path.rename(moved / "config.xml")
        core.env["STHOMEDIR"] = str(moved)
        core.start()
        self.assertTrue(core.ready()["state"]["connection"]["online"])
        core.request("shutdown", "2")
        core.finish()

    def test_configuration_errors(self):
        for name, text, message in [
            ("malformed", "<configuration><gui>", "Syncthing configuration is malformed"),
            (
                "no key",
                "<configuration><gui><address>127.0.0.1:1</address></gui></configuration>",
                "Syncthing API credential is missing",
            ),
            (
                "bad address",
                "<configuration><gui><address>nope</address><apikey>k</apikey></gui></configuration>",
                "Syncthing GUI address is invalid",
            ),
            (
                "tls without cert",
                '<configuration><gui tls="true"><address>127.0.0.1:1</address><apikey>k</apikey></gui></configuration>',
                "local HTTPS certificate was not found",
            ),
            ("large", "<configuration>" + " " * (1 << 20) + "</configuration>", "Syncthing configuration is too large"),
        ]:
            with self.subTest(name):
                core = self.core(start=False)
                core.config_path.write_text(text)
                core.start()
                self.assertEqual(core.finish(), 1)
                self.assertEqual(core.frames, [])
                self.assertIn(message.encode(), core.stderr)


class TlsTests(CoreTest):
    def test_pinned_certificate(self):
        core, _ = self.folder_core(tls=True)
        state = core.ready()["state"]
        self.assertTrue(state["connection"]["online"], state["connection"])
        self.assertEqual(state["connection"]["endpoint"], "https://" + core.syncthing.address)
        self.assertEqual(state["webUi"]["url"], "https://" + core.syncthing.address)
        core.request("shutdown", "1")
        core.finish()
        self.assert_secret_free(core)

    def test_another_certificate_is_refused_before_the_key_is_sent(self):
        core = self.core(tls=True, start=False)
        fake_syncthing.make_certificate(str(core.home), "other")
        (core.home / "other-cert.pem").replace(core.config_dir / "https-cert.pem")
        core.start()
        state = core.ready()["state"]
        self.assertEqual(state["connection"]["error"], {"code": "tls", "message": "Syncthing TLS verification failed"})
        self.assertFalse(state["connection"]["online"])
        self.assertEqual(core.syncthing.calls(), [])
        core.request("shutdown", "1")
        core.finish()

    def test_plain_http_to_a_tls_server_fails_cleanly(self):
        core = self.core(tls=True, start=False)
        core.write_config(tls=False, address=core.syncthing.address)
        core.start()
        state = core.ready()["state"]
        self.assertFalse(state["connection"]["online"])
        self.assertIn(state["connection"]["error"]["code"], ("connection", "schema", "http"))
        core.request("shutdown", "1")
        core.finish()


TIMEOUT = {"code": "timeout", "message": "request timed out"}
# Syncshell's Go transport: 8 s from the written request to the response
# headers, 15 s for a whole request. Slack covers a loaded host and qemu.
HEADER_BOUND, REQUEST_BOUND, EARLY, SLACK = 8.0, 15.0, 0.5, 2.5
# 3 s to connect (Go's net.Dialer.Timeout).
DIAL_BOUND = 3.0


class DeadlineTests(CoreTest):
    """A peer that trickles a reply a piece at a time (each piece well inside
    any per-read timeout) is cut off at Go's absolute bounds, over HTTP and
    HTTPS, and the requests queued behind it still run."""

    def trickle_core(self, mode, gap, tls):
        core = self.core(tls=tls, start=False)
        certificate = (str(core.config_dir / "https-cert.pem"), str(core.config_dir / "https-key.pem")) if tls else None
        trickler = fake_syncthing.Trickler(mode, gap, tls=certificate)
        self.addCleanup(trickler.close)
        core.write_config(tls=tls, address=trickler.address)
        return core

    def assert_bound(self, took, bound, what):
        self.assertGreaterEqual(took, bound - EARLY, f"{what} after {took:.2f}s")
        self.assertLess(took, bound + SLACK, f"{what} after {took:.2f}s")

    def test_a_trickling_peer_ends_at_the_absolute_bounds(self):
        # A trickled header ends at the header bound, a trickled body at the
        # request bound. The four cores run at the same time.
        cases = [
            (mode, tls, self.trickle_core(mode, gap, tls), bound)
            for mode, gap, bound in (("header", 2, HEADER_BOUND), ("body", 1, REQUEST_BOUND))
            for tls in (False, True)
        ]
        for _, _, core, _ in cases:
            core.start()
            # The core reads stdin only after its first snapshot, so this
            # waits in the pipe behind the request that trickles.
            core.send({"v": 2, "type": "shutdown", "id": "1"})
        for mode, tls, core, bound in cases:
            with self.subTest(mode=mode, tls=tls):
                state = core.ready(timeout=bound + 10)["state"]
                self.assertEqual(state["connection"]["error"], TIMEOUT)
                self.assertFalse(state["connection"]["online"])
                self.assert_bound(core.arrived - core.started, bound, "initial snapshot")
                snapshot_at = core.arrived
                result = core.wait_for(lambda f: f["type"] == "result" and f["id"] == "1", 5, "shutdown")
                self.assertTrue(result["ok"], result)
                self.assertLess(core.arrived - snapshot_at, 1.5)
                self.assertEqual(core.finish(), 0)
                self.assert_secret_free(core)

    def test_a_silent_dns_server_ends_at_the_dial_bound(self):
        # The lookup of a host name counts against the dial bound: a DNS
        # server that never answers (the resolver itself would wait 30 s)
        # ends the first refresh at 3 s, and a shutdown sent while it
        # waits is answered right after.
        core = self.core(start=False)
        prefix = silent_dns_prefix(core.home)
        if prefix is None:
            self.skipTest("unprivileged user and network namespaces are unavailable")
        core.wrapper = prefix
        core.write_config(address="syncthing-gui.example.test:8384")
        core.start()
        self.assertEqual(core.next_frame()["type"], "hello")
        core.send({"v": 2, "type": "shutdown", "id": "1"})
        state = core.snapshot(lambda s: s["connection"]["phase"] in ("ready", "error"), DIAL_BOUND + 10)["state"]
        self.assertEqual(state["connection"]["error"], TIMEOUT)
        self.assertFalse(state["connection"]["online"])
        self.assert_bound(core.arrived - core.started, DIAL_BOUND, "initial snapshot")
        snapshot_at = core.arrived
        result = core.wait_for(lambda f: f["type"] == "result" and f["id"] == "1", 5, "shutdown")
        self.assertTrue(result["ok"], result)
        self.assertLess(core.arrived - snapshot_at, 1.5)
        self.assertEqual(core.finish(), 0)
        self.assert_secret_free(core)


class UnixSocketTests(CoreTest):
    def test_an_escaped_unix_url_reaches_its_socket(self):
        # unix:///…/gui%20socket is the socket "gui socket" (Go's URL.Path is
        # decoded); the raw absolute path is taken as it is.
        for escaped in (True, False):
            with self.subTest(escaped=escaped):
                core = self.core(start=False, unix_socket="gui socket")
                path = core.syncthing.address
                self.assertTrue(path.endswith("/gui socket"), path)
                core.write_config(address="unix://" + quote(path) if escaped else path)
                core.start()
                state = core.ready()["state"]
                self.assertEqual(state["connection"]["phase"], "ready", state["connection"])
                self.assertEqual(state["connection"]["endpoint"], "unix://" + path)
                self.assertTrue(state["connection"]["online"])
                self.assertTrue(core.syncthing.calls("GET", "/rest/system/status"))
                # No web UI link: the panel would open a unix:// URL in a browser.
                self.assertNotIn("url", state["webUi"])
                self.assertEqual(core.finish(), 0)
                self.assert_secret_free(core)


class FailureTests(CoreTest):
    def test_wrong_api_key_is_unauthorized_and_external(self):
        core, _ = self.folder_core(api_key="wrong-Canary-1234")
        state = core.ready()["state"]
        self.assertEqual(
            state["connection"]["error"], {"code": "unauthorized", "message": "Syncthing authorization failed"}
        )
        self.assertEqual((state["connection"]["healthy"], state["connection"]["authorized"]), (True, False))
        self.assertEqual(state["lifecycle"]["classification"], "external")
        core.request("shutdown", "1")
        core.finish()
        self.assertNotIn(b"Canary", core.raw + core.stderr)

    def test_bad_responses(self):
        for override, code, message in [
            ((200, b"not json"), "schema", "Syncthing response is invalid"),
            ((200, b'{"myID":"x"} {}'), "schema", "Syncthing response contains trailing data"),
            ((200, b'{"myID":"  "}'), "schema", "Syncthing identity is missing"),
            ((500, b"boom"), "http", "Syncthing returned HTTP 500"),
            ((200, b" " * ((8 << 20) + 1)), "schema", "Syncthing response is too large"),
        ]:
            with self.subTest(message):
                core = self.core(start=False)
                core.syncthing.overrides[("GET", "/rest/system/status")] = override
                core.start()
                state = core.ready()["state"]
                self.assertEqual(state["connection"]["error"], {"code": code, "message": message})
                core.request("shutdown", "1")
                core.finish()

    def test_a_folder_without_its_error_model_is_still_shown(self):
        # A 404 from /rest/folder/errors (the folder's error model is not
        # ready yet) reads as "no errors": not a failure, not the old list.
        core, _ = self.folder_core()
        core.ready()
        with core.syncthing.lock:
            core.syncthing.state["errors"]["docs"] = [{"path": "a", "error": "denied"}]
        self.assertTrue(core.request("refresh", "1")["ok"])
        self.assertEqual(core.latest_state()["counts"]["folderProblems"], 1)
        core.syncthing.overrides[("GET", "/rest/folder/errors")] = (404, b"not ready")
        result = core.request("refresh", "2")
        self.assertTrue(result["ok"], result)
        state = core.latest_state()
        self.assertEqual([f["id"] for f in state["folders"]], ["docs"])
        self.assertIsNone(state["folders"][0]["status"]["errors"])
        self.assertEqual(state["counts"]["folderProblems"], 0)
        self.assertTrue(state["connection"]["online"])
        core.request("shutdown", "3")
        core.finish()

    def test_health_failure_is_reported_and_recovers(self):
        core = self.core(start=False)
        core.syncthing.overrides[("GET", "/rest/noauth/health")] = (200, {"status": "starting"})
        core.start()
        state = core.ready()["state"]
        self.assertEqual(
            state["connection"]["error"], {"code": "schema", "message": "Syncthing health response is invalid"}
        )
        del core.syncthing.overrides[("GET", "/rest/noauth/health")]
        core.request("refresh", "1")
        self.assertTrue(core.latest_state()["connection"]["online"])
        core.request("shutdown", "2")
        core.finish()


class VersionTests(CoreTest):
    def version_reads(self, core):
        return len(core.syncthing.calls("GET", "/rest/system/version"))

    def test_our_version_is_read_once_per_connection_and_restart(self):
        core = self.core()
        self.assertEqual(core.ready()["state"]["identity"]["version"], "v2.0.10")
        for request_id in ("1", "2"):
            self.assertTrue(core.request("refresh", request_id)["ok"])
        self.assertEqual(self.version_reads(core), 1, "not on every refresh")
        # A Syncthing restart (new start time) reads it again.
        with core.syncthing.lock:
            core.syncthing.state["startTime"] = "2026-11-12T09:00:00+01:00"
            core.syncthing.state["version"] = "v2.0.11"
        core.request("refresh", "3")
        self.assertEqual(self.version_reads(core), 2)
        self.assertEqual(core.latest_state()["identity"]["version"], "v2.0.11")
        # So does a new connection after the old one failed.
        core.syncthing.overrides[("GET", "/rest/noauth/health")] = (503, b"down")
        core.request("refresh", "4")
        self.assertFalse(core.latest_state()["connection"]["online"])
        del core.syncthing.overrides[("GET", "/rest/noauth/health")]
        core.request("refresh", "5")
        self.assertTrue(core.latest_state()["connection"]["online"])
        self.assertEqual(self.version_reads(core), 3)
        core.request("shutdown", "6")
        core.finish()

    def test_a_failed_version_read_hides_it_without_an_error(self):
        core = self.core(start=False)
        core.syncthing.overrides[("GET", "/rest/system/version")] = (500, b"boom")
        core.start()
        state = core.ready()["state"]
        self.assertEqual(state["connection"]["phase"], "ready")
        self.assertTrue(state["connection"]["online"])
        self.assertNotIn("error", state["connection"])
        self.assertEqual(state["identity"], {"deviceId": LOCAL_ID})
        core.request("shutdown", "1")
        core.finish()

    def test_untrusted_versions_are_bounded_and_printable(self):
        # Both Syncthing's own version and a peer's go through the bound:
        # trimmed, too long, a control sequence. types.rs
        # `versions_are_short_printable_ascii_or_nothing` has the other rows.
        cases = [
            (" v2.0.9 ", "v2.0.9"),
            ("v" + "1" * 40, None),
            ("v2.0.9\x1b[31m", None),
        ]
        for value, expected in cases:
            with self.subTest(value=value):
                core = self.core(start=False)
                with core.syncthing.lock:
                    core.syncthing.state["connections"][PEER_ID]["clientVersion"] = value
                    core.syncthing.state["version"] = value
                core.start()
                state = core.ready()["state"]
                peer = next(d for d in state["devices"] if d["id"] == PEER_ID)
                self.assertTrue(peer["connected"])
                self.assertEqual(peer.get("version"), expected)
                self.assertEqual(state["identity"].get("version"), expected)
                core.request("shutdown", "1")
                core.finish()


class EventTests(CoreTest):
    def test_item_events_become_rotating_activity(self):
        core, docs = self.folder_core()
        core.ready()
        core.syncthing.wait_polling()
        core.syncthing.add_event(
            "ItemStarted", {"folder": "docs", "item": "notes/plan.md", "type": "file", "action": "update"}
        )
        state = core.snapshot(lambda s: s["activity"]["files"], timeout=10)["state"]
        self.assertEqual(
            state["activity"]["files"],
            [{"folderId": "docs", "path": "notes/plan.md", "action": "syncing", "detail": "plan.md"}],
        )
        self.assertEqual(state["activity"]["current"]["detail"], "plan.md")
        core.syncthing.add_event("ItemStarted", {"folder": "docs", "item": "old.txt", "action": "delete"})
        state = core.snapshot(lambda s: len(s["activity"]["files"] or []) == 2, timeout=10)["state"]
        self.assertIn(
            {"folderId": "docs", "path": "old.txt", "action": "removing", "detail": "Removing old.txt"},
            state["activity"]["files"],
        )
        core.syncthing.add_event("ItemFinished", {"folder": "docs", "item": "old.txt"})
        core.syncthing.add_event("ItemFinished", {"folder": "docs", "item": "notes/plan.md"})
        core.snapshot(lambda s: s["activity"]["files"] is None, timeout=10)
        core.request("shutdown", "1")
        core.finish()

    def test_config_events_rehydrate(self):
        core, _ = self.folder_core()
        core.ready()
        core.syncthing.wait_polling()
        core.syncthing.state["devices"][0]["name"] = "renamed by event"
        core.syncthing.add_event("ConfigSaved", {})
        core.snapshot(lambda s: any(d["name"] == "renamed by event" for d in s["devices"]), timeout=10)
        events = core.syncthing.calls("GET", "/rest/events")
        self.assertTrue(events)
        # The long poll asks for config changes, with its stated bounds.
        query = parse_qs(urlsplit(events[-1]["target"]).query)
        self.assertEqual(query["limit"], ["256"])
        self.assertEqual(query["timeout"], ["1"])
        self.assertIn("ConfigSaved", query["events"][0].split(","))
        core.request("shutdown", "1")
        core.finish()

    def test_unreachable_events_mark_the_snapshot_stale(self):
        core, _ = self.folder_core()
        core.ready()
        core.syncthing.overrides[("GET", "/rest/events")] = (500, b"down")
        core.snapshot(lambda s: s["connection"]["fresh"] is False, timeout=10)
        del core.syncthing.overrides[("GET", "/rest/events")]
        core.snapshot(lambda s: s["connection"]["fresh"] is True, timeout=15)
        core.request("shutdown", "1")
        core.finish()


class LiveConfigTests(CoreTest):
    def test_the_probe_interval_drives_lifecycle_probes(self):
        core, _ = self.folder_core(flags=["--probe-interval-seconds", "1"] + PLUGIN_FLAGS[2:])
        core.ready()
        core.set_systemctl({"ActiveState": "activating"})
        state = core.snapshot(lambda s: s["lifecycle"]["activeState"] == "activating", timeout=10)["state"]
        self.assertTrue(state["lifecycle"]["active"])
        core.request("shutdown", "1")
        core.finish()


if __name__ == "__main__":
    unittest.main()
