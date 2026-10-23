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

test("native folder snapshots preserve paused state, sharing and errors", () => {
  const facade = model("hosts/omarchy/models/FacadeModel.js");
  const folders = [{ id: "work", path: "/tmp/work", paused: true,
    devices: [{ id: "remote" }], status: { state: "error", errors: ["offline"], pullErrors: 1 } }];
  assert.equal(facade.folders(folders)[0].paused, true);
  assert.equal(facade.folders(folders)[0].devices[0].deviceID, "remote");
  assert.equal(facade.folderStatuses(folders).work.errors, 1);
  assert.equal(facade.folderStatuses(folders).work.pullErrors, 1);
});

test("settings reject unsupported future versions instead of silently resetting", () => {
  const settings = model("hosts/omarchy/models/SettingsModel.js");
  const defaults = fs.readFileSync(path.join(__dirname, "../hosts/omarchy/config/settings.toml"), "utf8");
  assert.equal(settings.parse(defaults).error, "");
  assert.ok(settings.parse(defaults.replace("version = 2", "version = 999")).error);
});

test("native protocol fails closed on malformed input or wrong major version", () => {
  const source = fs.readFileSync(path.join(__dirname, "../shared/CoreProcess.qml"), "utf8");
  const context = vm.createContext({ protocolReady: false, maxLineLength: 8388607,
    error: "",
    acceptHello() { throw new Error("invalid input must not reach hello"); } });
  context.failProtocol = message => { context.error = message; };
  for (const name of ["utf8Length", "handleLine"]) {
    const start = source.indexOf(`  function ${name}(`);
    const end = source.indexOf("\n  }", start) + 4;
    assert.ok(start >= 0);
    vm.runInContext(source.slice(start, end), context);
  }
  context.handleLine("invalid");
  assert.match(context.error, /not valid JSON/);
  context.handleLine(JSON.stringify({ v: 99, type: "hello" }));
  assert.match(context.error, /incompatible/);
});

test("bar tooltips use the public scoped API", () => {
  const source = fs.readFileSync(path.join(__dirname, "../hosts/omarchy/OmarchyPanel.qml"), "utf8");
  const start = source.indexOf("  function showBarTooltip(");
  const end = source.indexOf("\n  }", start) + 4;
  const calls = [];
  const button = { tooltipHovered: true };
  const context = vm.createContext({ button, tooltip: "Syncthing: Up to date",
    bar: Object.freeze({ showTooltip(target, text) { calls.push([target, text]); } }) });
  vm.runInContext(source.slice(start, end), context);
  context.showBarTooltip();
  assert.equal(calls.length, 1);
  assert.equal(calls[0][1], "Syncthing: Up to date");
});

test("original panel totals include this computer", () => {
  const panel = model("models/PanelModel.js");
  assert.equal(panel.deviceCounts(null).connected, 0);
  const counts = panel.deviceCounts({localDeviceId: "local", connectedDeviceCount: 2, deviceCount: 5});
  assert.equal(counts.connected, 3);
  assert.equal(counts.total, 6);
  assert.equal(panel.deviceCounts({localDeviceId: "", connectedDeviceCount: 0, deviceCount: 0}).total, 0);
});

test("native snapshots feed the original folder cards without losing health", () => {
  const facade = model("hosts/omarchy/models/FacadeModel.js");
  const panel = model("models/PanelModel.js");
  const raw = [{id: "work", label: "Work", path: "/home/user/work", paused: false,
    devices: [{id: "local"}, {id: "remote"}],
    status: {state: "error", errors: ["Permission denied"], globalFiles: 12, globalBytes: 2048}}];
  const rows = panel.buildFolderRows({folders: facade.folders(raw), folderStatuses: facade.folderStatuses(raw), localDeviceId: "local"}, "/home/user");
  assert.equal(rows[0].label, "work");
  assert.equal(rows[0].configuredLabel, "Work");
  assert.equal(panel.folderState(rows[0], "", false), "ERROR");
  assert.equal(panel.total(rows, "globalFiles"), 12);
});

test("idle outstanding items stay critical through the native adapter and both presentations", () => {
  const facade = model("hosts/omarchy/models/FacadeModel.js");
  const panel = model("models/PanelModel.js");
  const overview = model("models/OverviewModel.js");
  const translations = require("./i18n.cjs");
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
  assert.equal(panel.folderState(folders[0], "work", true), "OUT OF SYNC");
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
  assert.equal(translations.text(summary.title, "nb"), "Ute av synk");

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

test("bar icon uses urgent color for folder errors and prioritizes them over device warnings", () => {
  const source = fs.readFileSync(path.join(__dirname, "../Panel.qml"), "utf8");
  const panel = model("models/PanelModel.js");
  const health = source.match(/readonly property bool hasProblems: ([\s\S]*?)\n  readonly property/)[1];
  const color = source.match(/readonly property color syncthingIconColor: \{([\s\S]*?)\n  \}/)[1];
  const context = vm.createContext({PanelModel: panel, urgent: "red", foreground: "white",
    hasTooFewDevices: true, Color: {bar: {background: {hslLightness: 0.1}}}});
  for (const [service, folders, expected] of [
    [{folderProblemCount: 0}, [{problem: true, outOfSync: true}], "red"],
    [{folderProblemCount: 1}, [], "red"],
    [{hasConflicts: true}, [], "red"],
    [{phase: "error"}, [], "red"],
    [{folderProblemCount: 0}, [{problem: false}], "#ebcb8b"],
    [null, [], "#ebcb8b"]
  ]) {
    context.syncthing = service;
    context.folderRows = folders;
    context.hasProblems = vm.runInContext(health, context);
    assert.equal(vm.runInContext(`(function() {${color}})()`, context), expected);
  }
  context.hasTooFewDevices = false;
  assert.equal(vm.runInContext(`(function() {${color}})()`, context), "white");
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

test("pause and resume preserve the folder configuration and use accurate status", () => {
  const panel = model("models/PanelModel.js");
  assert.equal(panel.folderState({ id: "work", paused: true }, "", false), "PAUSED");
  const source = fs.readFileSync(path.join(__dirname, "../Service.qml"), "utf8");
  const start = source.indexOf("  function setFolderPaused(");
  const end = source.indexOf("\n  }", start) + 4;
  const calls = [];
  const context = vm.createContext({
    folderLabel: () => "Work",
    tr: (key, values) => key.replace(/\{(\w+)\}/g, (match, name) => values?.[name] ?? match),
    runFolderAction: (...args) => calls.push(args),
  });
  context.root = context;
  vm.runInContext(source.slice(start, end), context);
  context.setFolderPaused("work", true);
  context.setFolderPaused("work", false);
  assert.deepEqual(calls.map(call => call[0]), ["folder.pause", "folder.resume"]);
  for (const call of calls) {
    assert.equal(call[2], "work");
    assert.deepEqual(Object.keys(call[3]), ["folderId"]);
    assert.match(call[4], /sharing, and local files are unchanged/);
  }
});

test("local-only folder notices retain private labels only in the panel", () => {
  const service = fs.readFileSync(path.join(__dirname, "../Service.qml"), "utf8");
  const adapter = fs.readFileSync(path.join(__dirname, "../hosts/omarchy/OmarchyService.qml"), "utf8");
  const notices = [];
  const context = vm.createContext({
    folderLabel: () => 'private-Canary-847 📁',
    tr: (key, values) => key.replace(/\{(\w+)\}/g, (match, name) => values?.[name] ?? match),
    UiConstants: {NOTICE_VISIBLE_MS: 1000}, folderMutationNotice: "",
    PluginLanguage: {FoamyI18n: {t: key => key}}, settings: {},
    noticeTimer: {restart() {}},
    notificationSender: {send(message, visibleMs) { notices.push([message, visibleMs]); return true; }},
  });
  context.root = context;
  context.runFolderAction = (...args) => context.publishFolderNotice(args[4]);
  for (const [source, name] of [[service, "setFolderPaused"], [adapter, "notify"], [adapter, "publishFolderNotice"]]) {
    const start = source.indexOf(`  function ${name}(`);
    const end = source.indexOf("\n  }", start) + 4;
    assert.ok(start >= 0);
    vm.runInContext(source.slice(start, end), context);
  }
  context.setFolderPaused("local-only", true);
  assert.match(context.folderMutationNotice, /Paused private-Canary-847 📁/);
  context.setFolderPaused("local-only", false);
  assert.match(context.folderMutationNotice, /Resumed private-Canary-847 📁/);
  assert.equal(notices.length, 2);
  for (const [text] of notices) {
    assert.equal(text, "Syncthing status changed. Open the panel for details.");
    assert.doesNotMatch(text, /private-Canary-847/);
  }
  context.PluginLanguage.FoamyI18n.t = key => require("./i18n.cjs").text(key, "no");
  context.notify("Failed to scan /private/Canary-847 on device PrivateDevice", 12000);
  assert.equal(notices[2][0], "Syncthing-statusen er endret. Åpne panelet for detaljer.");
  assert.equal(notices[2][1], 12000);
});

test("unlink pauses before removing configuration and stops safely on errors", () => {
  function fixture(paused = false) {
    const requests = [];
    const context = vm.createContext({
      online: true, folderMutationBusy: false, folderMutationError: "",
      folderMutationAction: "", folderMutationId: "", notice: "",
      configuredFolder: () => ({ id: "work", paused }),
      folderLabel: () => "Work",
    tr: (key, values) => key.replace(/\{(\w+)\}/g, (match, name) => values?.[name] ?? match),
      clearFolderMutationMessage() {},
      actionError: (error, fallback) => error?.message || fallback,
      core: { action: (name, args, callback) => {
        requests.push({ name, args, callback });
        return String(requests.length);
      } },
    });
    context.root = context;
    context.failFolderAction = (error, fallback) => {
      context.folderMutationBusy = false;
      context.folderMutationError = error?.message || fallback;
    };
    context.finishFolderAction = (action, id, notice) => {
      context.folderMutationBusy = false;
      context.notice = notice;
    };
    const source = fs.readFileSync(path.join(__dirname, "../Service.qml"), "utf8");
    for (const name of ["unlinkFolder", "removeUnlinkedFolder"]) {
      const start = source.indexOf(`  function ${name}(`);
      const end = source.indexOf("\n  }", start) + 4;
      vm.runInContext(source.slice(start, end), context);
    }
    return { context, requests };
  }
  const active = fixture();
  assert.equal(active.context.unlinkFolder("work"), true);
  assert.equal(active.requests[0].name, "folder.pause");
  assert.equal(active.context.folderMutationBusy, true);
  assert.equal(active.context.unlinkFolder("another"), false);
  active.requests[0].callback(true);
  assert.equal(active.requests[1].name, "folder.forget");
  assert.equal(active.context.folderMutationBusy, true);
  assert.deepEqual(Object.keys(active.requests[1].args), ["folderId"]);
  active.requests[1].callback(true);
  assert.equal(active.context.folderMutationBusy, false);
  assert.match(active.context.notice, /Local files were kept/);

  const paused = fixture(true);
  paused.context.unlinkFolder("work");
  assert.equal(paused.requests[0].name, "folder.forget");

  const pauseFailure = fixture();
  pauseFailure.context.unlinkFolder("work");
  pauseFailure.requests[0].callback(false, null, { message: "Cannot pause" });
  assert.equal(pauseFailure.requests.length, 1);
  assert.equal(pauseFailure.context.folderMutationError, "Cannot pause");
  assert.equal(pauseFailure.context.folderMutationBusy, false);

  const removalFailure = fixture();
  removalFailure.context.unlinkFolder("work");
  removalFailure.requests[0].callback(true);
  removalFailure.requests[1].callback(false, null, { message: "Cannot remove" });
  assert.match(removalFailure.context.folderMutationError, /remains paused/);
  assert.equal(removalFailure.context.folderMutationBusy, false);

  const offline = fixture();
  offline.context.online = false;
  assert.equal(offline.context.unlinkFolder("work"), false);
  assert.equal(offline.requests.length, 0);
});
