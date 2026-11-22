const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
function model(file) {
  const context = vm.createContext({});
  vm.runInContext(fs.readFileSync(path.join(__dirname, "..", file), "utf8").replace(/^\.pragma library\s*/, ""), context);
  return context;
}

test("native snapshots keep paused state, sharing and errors, and read null lists as empty", () => {
  const facade = model("service/Facade.js");
  const folders = [{ id: "work", path: "/tmp/work", paused: true,
    devices: [{ id: "remote" }], status: { state: "error", errors: ["offline"], pullErrors: 1 } }];
  assert.equal(facade.folders(folders)[0].paused, true);
  assert.equal(facade.folders(folders)[0].devices[0].deviceID, "remote");
  assert.equal(facade.folderStatuses(folders).work.errors, 1);
  assert.equal(facade.folderStatuses(folders).work.pullErrors, 1);
  // The core sends null for empty lists and leaves out a folder's status.
  const plain = value => JSON.parse(JSON.stringify(value));
  assert.deepEqual(plain(facade.folders(null)), []);
  assert.deepEqual(plain(facade.devices(null)), []);
  assert.deepEqual(plain(facade.folderStatuses(null)), {});
  const bare = plain(facade.folders([{ id: "a", devices: null }]))[0];
  assert.deepEqual(bare.devices, []);
  const status = plain(facade.folderStatuses([{ id: "a", status: null }])).a;
  assert.equal(status.state, "unknown");
  assert.equal(status.errors, 0);
  assert.deepEqual(status.errorDetails, []);
  assert.equal(status.needTotalItems, 0);
  assert.equal(facade.folderStatus({ errors: null, pullErrors: null }).errors, 0);
  assert.equal(facade.folderStatus({ errors: null, pullErrors: null }).pullErrors, 0);
  assert.deepEqual(plain(facade.syncingFiles({ files: null })), []);
  assert.deepEqual(plain(facade.syncingFiles(null)), []);
  // Start and stop show only for the authorized unit with a control.
  for (const [lifecycle, available] of [
    [{ targetMatch: true, canControl: true }, true],
    [{ targetMatch: true, canStart: true }, true],
    [{ targetMatch: false, canControl: true }, false],
    [{ targetMatch: true }, false],
    [null, false],
  ]) assert.equal(facade.lifecyclePresentation(lifecycle).available, available, JSON.stringify(lifecycle));
  assert.equal(facade.truncated(null), false);
  assert.equal(facade.truncated({ folderDevices: 2 }), true);
  assert.equal(facade.truncated({ folders: 0 }), false);
});

test("every list the core cuts shows the panel-limits warning", () => {
  const facade = model("service/Facade.js");
  // The keys the core's Truncation struct sends (native/src/core/types.rs).
  const source = fs.readFileSync(path.join(__dirname, "..", "native/src/core/types.rs"), "utf8");
  const struct = source.match(/pub struct Truncation \{([^}]*)\}/)[1];
  const keys = [...struct.matchAll(/(?:rename = "(\w+)"[^\n]*\n\s*)?pub (\w+): i64/g)].map(m => m[1] || m[2]);
  // Every field is read, in any order; a field the pattern misses fails here.
  assert.equal(keys.length, struct.match(/\bpub \w+:/g).length, JSON.stringify(keys));
  for (const key of keys) {
    assert.equal(facade.truncated({ [key]: 1 }), true, key);
    assert.equal(facade.truncated({ [key]: 0 }), false, key);
  }
});

test("original panel totals include this computer", () => {
  const panel = model("models/PanelModel.js");
  assert.equal(panel.deviceCounts(null).connected, 0);
  const counts = panel.deviceCounts({localDeviceId: "local", connectedDeviceCount: 2, deviceCount: 5});
  assert.equal(counts.connected, 3);
  assert.equal(counts.total, 6);
  assert.equal(panel.deviceCounts({localDeviceId: "", connectedDeviceCount: 0, deviceCount: 0}).total, 0);
});

test("the header counts only the other devices, like the Devices tab", () => {
  const panel = model("models/PanelModel.js");
  const tr = (key, values) => key.replace(/\{(\w+)\}/g, (m, k) => String(values[k]));
  const service = {online: true, localDeviceId: "local", connectedDeviceCount: 2, deviceCount: 2};
  assert.equal(panel.remoteDeviceLine(service, tr), "2 / 2 devices online");
  assert.equal(panel.remoteDeviceLine({...service, connectedDeviceCount: 1}, tr), "1 / 2 devices online");
  // No other devices: no count (the tab says 0); offline: the hero says why.
  assert.equal(panel.remoteDeviceLine({...service, connectedDeviceCount: 0, deviceCount: 0}, tr), "");
  assert.equal(panel.remoteDeviceLine({...service, online: false}, tr), "");
  assert.equal(panel.remoteDeviceLine(null, tr), "");
  // The minimum-devices warning still counts this computer.
  assert.equal(panel.deviceCounts(service).connected, 3);
});

test("native snapshots feed the original folder cards without losing health", () => {
  const facade = model("service/Facade.js");
  const panel = model("models/PanelModel.js");
  const raw = [{id: "work", label: "Work", path: "/home/user/work", paused: false,
    devices: [{id: "local"}, {id: "remote"}],
    status: {state: "error", errors: ["Permission denied"], globalFiles: 12, globalBytes: 2048}}];
  const rows = panel.buildFolderRows({folders: facade.folders(raw), folderStatuses: facade.folderStatuses(raw), localDeviceId: "local"}, "/home/user");
  // Syncthing's configured label comes first, like its Web UI; the path stays the subtitle.
  assert.equal(rows[0].label, "Work");
  assert.equal(rows[0].configuredLabel, "Work");
  assert.equal(rows[0].pathName, "work");
  const unlabeled = panel.buildFolderRows({folders: facade.folders([{id: "abc", path: "~/Docs"}]), folderStatuses: {}}, "/home/user");
  assert.equal(unlabeled[0].label, "Docs");
  assert.equal(panel.total(rows, "globalFiles"), 12);
});

test("idle outstanding items stay critical through the native adapter and both presentations", () => {
  const facade = model("service/Facade.js");
  const panel = model("models/PanelModel.js");
  const overview = model("models/OverviewModel.js");
  const raw = [{id: "work", path: "/tmp/work", devices: [{id: "remote"}],
    status: {state: "idle", needTotalItems: 2, needBytes: 0, globalBytes: 2048,
      errors: [], pullErrors: 0}}];
  const service = {online: true, folders: facade.folders(raw),
    folderStatuses: facade.folderStatuses(raw),
    devices: [{deviceID: "remote", connected: true}],
    // The bundled core counts outstanding items as syncing, with no pull error.
    folderProblemCount: 0, syncingFolderCount: 1,
    syncActivityFolderId: "work", syncActivityDetail: "Earlier activity"};
  const folders = panel.buildFolderRows(service, "/tmp");
  assert.equal(folders[0].outOfSync, true);
  assert.equal(folders[0].problem, true);
  assert.equal(folders[0].syncing, false);
  assert.equal(panel.compactFolderState(folders[0], true, false, true), "Out of sync");
  assert.equal(panel.folderMeta(folders[0]), "Out of sync");
  const summary = panel.compactSummary(service, folders);
  assert.equal(summary.title, "Out of sync");
  assert.equal(summary.progress, -1);
  const result = overview.rows(service, folders, key => key, String, String);
  assert.equal(result.folders[0].state, "Out of sync");
  assert.equal(result.folders[0].progress, -1);
  assert.doesNotMatch(result.folders[0].detail, /Earlier activity/);
  assert.equal(result.devices[0].state, "Out of sync");
  assert.match(result.devices[0].detail, /locally/);

  service.folderStatuses.work.needTotalItems = 0;
  service.syncActivityDetail = "";
  const recovered = panel.buildFolderRows(service, "/tmp");
  assert.equal(recovered[0].problem, false);
  assert.equal(panel.compactSummary(service, recovered).title, "Up to date");
});

test("paused, queued, scanning and unknown snapshots do not become idle out-of-sync errors", () => {
  const panel = model("models/PanelModel.js");
  const overview = model("models/OverviewModel.js");
  for (const [props, expected] of [
    [{state: "idle", paused: true}, "Paused"],
    [{state: "sync-waiting"}, "Waiting to sync"],
    [{state: "sync-preparing"}, "Waiting to sync"],
    [{state: "syncing"}, "Syncing"],
    [{state: "scanning"}, "Scanning"],
    [{state: "unknown"}, "Waiting to sync"]
  ]) {
    const service = {online: true, folders: [{id: "work", paused: props.paused}],
      folderStatuses: {work: {state: props.state, needTotalItems: 1}}};
    const folders = panel.buildFolderRows(service, "/tmp");
    assert.equal(folders[0].outOfSync, false, props.state);
    assert.equal(folders[0].problem, false, props.state);
    assert.equal(overview.rows(service, folders, key => key, String, String).folders[0].state,
      expected, props.state);
  }
});

test("zero-work sync preparation does not flash as activity", () => {
  const panel = model("models/PanelModel.js");
  const service = {folders: [{id: "work", path: "/work"}], folderStatuses: {work: {state: "sync-preparing", needTotalItems: 0}}};
  assert.equal(panel.buildFolderRows(service, "/home/user")[0].syncing, false);
  service.folderStatuses.work.needTotalItems = 1;
  assert.equal(panel.buildFolderRows(service, "/home/user")[0].syncing, true);
});

test("compact progress measures queued bytes without claiming premature completion", () => {
  const panel = model("models/PanelModel.js");
  const service = {online: true, serviceActive: true};
  const folder = {label: "Documents", state: "syncing", syncing: true, globalBytes: 1000, needBytes: 280, needItems: 3};
  assert.equal(panel.compactSummary(service, [folder]).progress, 72);
  folder.needBytes = 1;
  assert.equal(panel.compactSummary(service, [folder]).progress, 99);
  folder.needBytes = 0;
  assert.equal(panel.compactSummary(service, [folder]).progress, -1);
  folder.needBytes = 1200;
  assert.equal(panel.compactSummary(service, [folder]).progress, 0);
});

test("compact summary preserves unavailable, paused, scanning, error and empty states", () => {
  const panel = model("models/PanelModel.js");
  const service = {online: true, serviceActive: true, serviceAvailable: true};
  const folder = {label: "Documents", state: "idle", globalBytes: 1000};
  assert.equal(panel.compactSummary(service, [folder]).progress, 100);
  for (const variant of [{paused: true}, {problem: true}, {scanning: true}, {state: "unknown"}]) {
    assert.equal(panel.compactSummary(service, [{...folder, ...variant}]).progress, -1);
  }
  assert.equal(panel.compactSummary(service, []).title, "No shared folders");
  assert.equal(panel.compactSummary({...service, online: false}, [folder]).progress, -1);
  assert.equal(panel.compactSummary({...service, hasConflicts: true}, [folder]).title, "Needs attention");
  assert.equal(panel.compactSummary({...service, serviceActive: false}, [folder]).title, "Syncing stopped");
  assert.equal(panel.compactFolderState(folder, false, false, false), "Unavailable");
  assert.equal(panel.compactFolderState(folder, false, true, false), "Stopped");
  assert.equal(panel.compactFolderState(folder, true, false, true), "Syncing");
  assert.equal(panel.compactSummary(null, []).progress, -1);
});

test("the pick-folder reply gives a path, a cancel or an error code", () => {
  const panel = model("models/PanelModel.js");
  const reply = (code, text) => JSON.parse(JSON.stringify(panel.folderPickReply(code, text)));
  assert.deepEqual(reply(0, '{"ok":true,"path":"/home/me/Sync"}\n'), { path: "/home/me/Sync" });
  assert.deepEqual(reply(0, 'noise\n{"ok":true,"cancelled":true}\n'), { cancelled: true });
  assert.deepEqual(reply(1, '{"ok":false,"code":"NO_SESSION_BUS","error":"no bus"}\n'), { error: "NO_SESSION_BUS" });
  assert.deepEqual(reply(1, '{"ok":false,"code":"PORTAL_UNAVAILABLE","error":"x"}'), { error: "PORTAL_UNAVAILABLE" });
  // The timeout backstop, garbage, a relative path, a code that isn't one.
  assert.deepEqual(reply(124, ""), { error: "PORTAL_FAILED" });
  assert.deepEqual(reply(137, ""), { error: "PORTAL_FAILED" });
  assert.deepEqual(reply(0, "not json"), { error: "FAILED" });
  assert.deepEqual(reply(0, '{"ok":true,"path":"Sync"}'), { error: "FAILED" });
  assert.deepEqual(reply(1, '{"ok":false,"code":"<b>x</b>"}'), { error: "FAILED" });
  assert.deepEqual(reply(1, '{"ok":true,"path":"/a"}'), { error: "FAILED" });
});

test("folder paths resolve as Syncthing's: ~ and relative paths from HOME", () => {
  // Shared by the folder rows, "Open folder" and the conflict scan.
  const panel = model("models/PanelModel.js");
  assert.equal(panel.resolveFolderPath("~", "/home/me"), "/home/me");
  assert.equal(panel.resolveFolderPath("~/Sync", "/home/me"), "/home/me/Sync");
  assert.equal(panel.resolveFolderPath("Sync/a", "/home/me"), "/home/me/Sync/a");
  assert.equal(panel.resolveFolderPath("/srv/Sync", "/home/me"), "/srv/Sync");
  assert.equal(panel.resolveFolderPath("~other/x", "/home/me"), "/home/me/~other/x");
  assert.equal(panel.resolveFolderPath("Sync", ""), "Sync");
});

test("encrypted shares are never offered as plain ones", () => {
  const panel = model("models/PanelModel.js");
  const plain = value => JSON.parse(JSON.stringify(value));
  const service = {
    localDeviceId: "L",
    devices: [{ deviceID: "L" }, { deviceID: "P", name: "Laptop" }, { deviceID: "U", untrusted: true }],
    pendingFolders: {
      plain: { offeredBy: { P: { label: "Photos" }, X: {} } },
      secret: { offeredBy: { P: { receiveEncrypted: true } } },
      // One encrypted offer makes the whole folder a Web UI task.
      mixed: { offeredBy: { P: {}, U: { remoteEncrypted: true } } },
    },
  };
  const offers = plain(panel.pendingOfferOptions(service));
  assert.deepEqual(offers.map(offer => offer.value), [JSON.stringify(["plain", "P"]), JSON.stringify(["plain", "X"])]);
  // Without a label the folder id names the offer; an unknown sender says so.
  assert.deepEqual(offers.map(offer => offer.label), ["Photos from Laptop", "plain from Unknown device"]);
  assert.equal(panel.encryptedPendingOfferCount(service), 2);
  const devices = plain(panel.deviceOptions(service));
  assert.deepEqual(devices.map(device => device.value), ["P", "U"]);
  assert.equal(devices[0].encrypted, false);
  assert.equal(devices[1].encrypted, true);
  assert.equal(devices[1].description, "Encrypted sharing requires the Web UI");
  assert.deepEqual(plain(panel.pendingOfferOptions(null)), []);
  assert.deepEqual(plain(panel.deviceOptions({ devices: null })), []);
});
