const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');
const model = vm.createContext({});
vm.runInContext(fs.readFileSync(path.join(__dirname, '../models/OverviewModel.js'), 'utf8').replace(/^\.pragma library\s*/, ''), model);
const tr = (key, args) => key.replace(/\{(\w+)\}/g, (match, key) => args?.[key] ?? match);
const service = {online: true, localDeviceId: 'local', devices: [
  {deviceID: 'local', name: 'Local'}, {deviceID: 'peer', name: 'Peer', connected: true},
  {deviceID: 'offline', name: 'Offline', connected: false}]};
const folder = {id: 'docs', label: 'Docs', path: '/tmp/docs', state: 'idle', devices: [{deviceID: 'local'}, {deviceID: 'peer'}], globalBytes: 1000, globalFiles: 12, needBytes: 0, needItems: 0};
const rows = (svc = service, folders = [folder]) => model.rows(svc, folders, tr, String, String);

test('both tabs share memberships, exclude self, and preserve stable identities', () => {
  const result = rows();
  assert.equal(result.devices.length, 2);
  assert.equal(result.folders[0].related.length, 1);
  assert.equal(result.devices[0].related[0].label, 'Docs');
  assert.equal(result.devices[0].state, 'Connected');
  assert.equal(result.folders[0].state, 'Up to date');
  assert.equal(result.devices[1].related.length, 0);
});
test('offline and truncated data never report connected or up to date', () => {
  const unavailable = rows({...service, online: false});
  assert.equal(unavailable.folders[0].state, 'Unavailable');
  assert.equal(unavailable.folders[0].related[0].state, 'Unavailable');
  assert.equal(unavailable.devices[0].state, 'Unavailable');
  assert.equal(rows(service, [{...folder, devices: [{deviceID: 'offline'}]}]).folders[0].state, 'Disconnected');
  assert.equal(rows(service, [{...folder, devices: [{deviceID: 'missing'}]}]).folders[0].state, 'Unavailable');
  assert.equal(rows(service, [{...folder, devices: []}]).folders[0].state, 'Up to date');
});
test('pending work and refreshes do not imply an active transfer', () => {
  const pending = rows({...service, refreshing: true}, [{...folder, needItems: 1, syncing: true}]);
  assert.equal(pending.folders[0].state, 'Waiting to sync');
  assert.equal(pending.devices[0].state, 'Connected');
  const active = rows(service, [{...folder, state: 'syncing', needBytes: 280, needItems: 1}]);
  assert.equal(active.folders[0].progress, 72);
  assert.equal(active.devices[0].state, 'Syncing');
  assert.match(active.devices[0].detail, /shared folder is syncing locally/);
  assert.equal(active.devices[1].state, 'Disconnected');
  assert.equal(rows(service, [{...folder, state: 'syncing', needItems: 1}]).folders[0].progress, -1);
});
test('local errors remain actionable and are not attributed to an offline device', () => {
  const failed = {...folder, problem: true, errorDetails: [{path: 'report.pdf', error: 'Permission denied'}]};
  const result = rows(service, [failed]);
  assert.equal(result.folders[0].state, 'Error');
  assert.match(result.folders[0].detail, /report.pdf: Permission denied/);
  assert.equal(result.devices[0].state, 'Error');
  assert.equal(rows(service, [{...failed, devices: [{deviceID: 'offline'}]}]).devices[1].state, 'Disconnected');
});
test('paused, scanning and unknown folder states remain distinct', () => {
  for (const [props, expected] of [[{paused:true}, 'Paused'], [{scanning:true}, 'Scanning'], [{state:'unknown'}, 'Checking']]) {
    assert.equal(rows(service, [{...folder, ...props}]).folders[0].state, expected);
  }
});

test('out-of-sync folders preserve errors and do not turn offline peers into error states', () => {
  const failed = {...folder, outOfSync: true, problem: true, needItems: 1,
    errorDetails: [{path: 'report.pdf', error: 'Permission denied'}]};
  const result = rows(service, [failed]);
  assert.equal(result.folders[0].state, 'Out of sync');
  assert.match(result.folders[0].detail, /report.pdf: Permission denied/);
  assert.equal(result.devices[0].state, 'Out of sync');
  const offline = rows(service, [{...failed, devices: [{deviceID: 'offline'}]}]);
  assert.equal(offline.folders[0].state, 'Out of sync');
  assert.equal(offline.devices[1].state, 'Disconnected');
  assert.equal(rows({...service, online: false}, [failed]).folders[0].state, 'Unavailable');
});

test('a connected peer shows its Syncthing version; a disconnected or offline one does not', () => {
  const versioned = {...service, devices: [
    {deviceID: 'local', name: 'Local', version: 'v2.0.10'},
    {deviceID: 'peer', name: 'Peer', connected: true, version: 'v2.0.9'},
    {deviceID: 'offline', name: 'Offline', connected: false, version: 'v1.27.0'}]};
  const result = rows(versioned);
  assert.equal(result.devices[0].subtitle, '1 shared folders · v2.0.9');
  assert.doesNotMatch(result.devices[1].subtitle, /v1\.27/);
  assert.doesNotMatch(rows({...versioned, online: false}).devices[0].subtitle, /v2\.0\.9/);
  // No version: the subtitle is the folder count alone.
  assert.doesNotMatch(rows().devices[0].subtitle, /·/);
});
