const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');
const I = require('./i18n.cjs');
const tokens = s => [...s.matchAll(/%[1-9][0-9]*|\{\w+\}/g)].map(m => m[0]).sort();
const forms = value => (typeof value === 'object' ? Object.values(value) : [value]);

function model(file) {
  const context = vm.createContext({});
  vm.runInContext(fs.readFileSync(path.join(__dirname, '..', file), 'utf8').replace(/^\.pragma library\s*/, ''), context);
  return context;
}

test('every English key has non-empty Norwegian text with the same placeholders', () => {
  // The product name is the one key that stays English.
  assert.deepEqual(Object.keys(I.en).filter(key => !Object.hasOwn(I.no, key)), ['Syncthing']);
  for (const [key, value] of Object.entries(I.en)) {
    const english = forms(value);
    if (typeof value === 'string') assert.equal(value, key, `English source key: ${key}`);
    if (!Object.hasOwn(I.no, key)) continue;
    assert.equal(typeof I.no[key], typeof value, `${key}: plural and plain entries match`);
    for (const form of forms(I.no[key])) {
      assert.notEqual(form, '', `${key}: empty Norwegian text`);
      assert.deepEqual(tokens(form), tokens(english[english.length - 1]), key);
    }
  }
});

test('every key the models return for display has a catalog entry', () => {
  const panel = model('models/PanelModel.js');
  const overview = model('models/OverviewModel.js');
  const keys = new Set();
  // Record what the models ask the translator for; return the key unchanged.
  const tr = key => { keys.add(key); return key; };
  const tn = key => { keys.add(key); return key; };
  // Values that come from the fixtures (labels, paths, service text), not from the models.
  const fixture = new Set(['Docs', '/home/me/Docs', 'Laptop', 'Earlier activity', 'Finding Syncthing']);
  const shown = value => { if (value && !fixture.has(value)) keys.add(value); };

  const ready = {online: true, serviceActive: true, serviceAvailable: true};
  const idle = {label: 'Docs', state: 'idle', globalBytes: 1000};
  const offline = state => ({online: false, installationState: state, summaryText: 'Finding Syncthing'});
  const summaries = [
    [null, []],
    [{serviceAvailable: true, serviceActive: false}, [idle]],
    [offline('missing'), []],
    [offline('incomplete'), []],
    [{...offline('incomplete'), installationBlocker: '/home/me/Docs'}, []],
    [offline('existing'), []],
    [ready, [{...idle, problem: true}]],
    [ready, [{...idle, problem: true, outOfSync: true}]],
    [{...ready, hasConflicts: true}, [idle]],
    [{...ready, hasConflicts: true}, [{...idle, conflict: true}]],
    [ready, []],
    [ready, [{...idle, state: 'syncing', syncing: true, needBytes: 200}]],
    [ready, [{...idle, state: 'syncing', syncing: true, needItems: 2}, {...idle, syncing: true, needItems: 1}]],
    [ready, [{...idle, state: 'scanning', scanning: true}]],
    [{...ready, syncActivityDetail: 'Earlier activity'}, [idle]],
    [ready, [{...idle, paused: true}]],
    [ready, [{...idle, paused: true}, idle]],
    [ready, [{...idle, state: 'unknown'}]],
    [ready, [idle]],
  ];
  for (const [service, rows] of summaries) {
    const summary = panel.compactSummary(service, rows, tr, tn);
    shown(summary.title);
    shown(summary.detail);
  }

  for (const [folder, online, stopped, activity] of [
    [idle, true, true, false],
    [idle, false, false, false],
    [{...idle, outOfSync: true}, true, false, false],
    [{...idle, problem: true}, true, false, false],
    [{...idle, conflict: true}, true, false, false],
    [{...idle, paused: true}, true, false, false],
    [{...idle, scanning: true}, true, false, false],
    [idle, true, false, true],
    [{...idle, state: 'unknown'}, true, false, false],
    [idle, true, false, false],
  ]) shown(panel.compactFolderState(folder, online, stopped, activity));

  // Folder rows built from a snapshot, one per state the overview can show.
  const folder = (id, status, extra = {}) =>
    ({id, label: 'Docs', path: '/home/me/Docs', devices: [{deviceID: 'P'}], ...extra, status});
  const raw = [
    folder('out', {state: 'idle', needTotalItems: 2}),
    folder('error', {state: 'error', errors: 1}),
    folder('conflict', {state: 'idle'}),
    folder('paused', {state: 'idle'}, {paused: true}),
    folder('scan', {state: 'scanning'}),
    folder('sync', {state: 'syncing', needTotalItems: 1, needBytes: 10, globalBytes: 100}),
    folder('wait', {state: 'sync-waiting', needTotalItems: 1}),
    folder('check', {state: 'unknown'}),
    folder('ok', {state: 'idle'}),
    folder('local', {state: 'idle'}, {devices: []}),
    folder('gone', {state: 'idle'}, {devices: [{deviceID: 'MISSING'}]}),
  ];
  const service = {
    online: true, localDeviceId: 'L',
    devices: [{deviceID: 'L'}, {deviceID: 'P', name: 'Laptop', connected: true}, {deviceID: 'Q', connected: false}],
    folders: raw.map(f => ({id: f.id, label: f.label, path: f.path, paused: f.paused, devices: f.devices})),
    folderStatuses: Object.fromEntries(raw.map(f => [f.id, f.status])),
    conflictFolderIds: {conflict: true},
  };
  // A folder whose only peer is offline, and a peer whose shared folders all sync.
  const offlinePeer = {...service, folders: [{...service.folders[8], devices: [{deviceID: 'Q'}]}]};
  const syncingPeer = {...service, folders: [service.folders[5]]};
  const outOfSyncPeer = {...service, folders: [service.folders[0]]};
  for (const svc of [service, offlinePeer, syncingPeer, outOfSyncPeer, {...service, online: false}]) {
    const result = overview.rows(svc, panel.buildFolderRows(svc, '/home/me'), tr, String, String, tn);
    for (const row of [...result.folders, ...result.devices]) {
      shown(row.state);
      for (const related of row.related) shown(related.state);
    }
  }

  // A table that stopped reaching the branches would pass with few keys.
  assert.ok(keys.size >= 25, `only ${keys.size} keys collected`);
  const missing = [...keys].filter(key => !Object.hasOwn(I.en, key));
  assert.deepEqual(missing, [], 'model keys missing from i18n/en.json');
});
