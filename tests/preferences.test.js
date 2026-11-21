const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');
function load(name) {
  const scope = vm.createContext({console});
  vm.runInContext(fs.readFileSync(path.join(__dirname, '..', name), 'utf8').replace(/^\.pragma library\s*/m, ''), scope);
  return scope;
}
const schema = require('../Preferences.js');
const prefs = require('../kit/node-loader.cjs').load('FoamyPrefs');
const manifest = require('../manifest.json');
const translations = require('./i18n.cjs');
const value = (settings, key) => prefs.value(schema.fields, settings, key);

test('the minimum-devices warning is off by default', () => {
  assert.equal(value({}, 'minimumConnectedDevices'), 0);
});

test('a stored serviceState from an older version loads and saves harmlessly', () => {
  // The setting did nothing and was removed; shell.json entries may still hold it.
  for (const schemaFields of [schema.fields, manifest.barWidget.schema]) {
    assert.equal(schemaFields.some(f => f.key === 'serviceState'), false);
  }
  assert.equal('serviceState' in manifest.barWidget.defaults, false);
  const stored = {id: 'foamy.syncthing', serviceState: 'disabled', refreshIntervalSec: 120};
  assert.deepEqual(JSON.parse(JSON.stringify(prefs.values(schema.fields, stored))),
                   JSON.parse(JSON.stringify(prefs.values(schema.fields, {refreshIntervalSec: 120}))));
  assert.deepEqual(Object.keys(prefs.storedProblems(schema.fields, stored)), []);
  const calls = [];
  const bar = {shell: {updateEntryInline(id, entry) { calls.push(entry); return true; }}};
  assert.equal(prefs.save(bar, 'foamy.syncthing', schema.fields, stored, 'probeIntervalSec', 30).ok, true);
  // Another key's save leaves the old value as it was, like any unknown key.
  assert.equal(calls[0].serviceState, 'disabled');
  assert.equal(calls[0].probeIntervalSec, 30);
});

test('the schema mirrors the manifest defaults and schema', () => {
  const defaults = manifest.barWidget.defaults;
  for (const field of schema.fields) assert.equal(defaults[field.key], field.defaultValue, field.key);
  assert.deepEqual(Object.keys(defaults).sort(), schema.fields.map(f => f.key).sort());
  for (const entry of manifest.barWidget.schema || []) {
    const field = schema.fields.find(f => f.key === entry.key);
    assert.ok(field, entry.key);
    for (const key of ['type', 'min', 'max', 'step', 'defaultValue']) assert.equal(entry[key], field[key], `${entry.key}.${key}`);
    assert.deepEqual(entry.options, field.options, `${entry.key}.options`);
  }
  // Every setting is in the manifest schema, also those without a control.
  assert.deepEqual(manifest.barWidget.schema.map(e => e.key).sort(), schema.fields.map(f => f.key).sort());
});

test('every static UI translation key has Norwegian text', () => {
  const files = fs.readdirSync(path.join(__dirname, '..'), {recursive: true})
    .filter(f => /\.(qml|js)$/.test(f) && !/^(kit|tests|preview|node_modules)\//.test(f));
  for (const file of files) {
    const source = fs.readFileSync(path.join(__dirname, '..', file), 'utf8');
    for (const match of source.matchAll(/\b(?:tr|t|tn)\(("(?:[^"\\]|\\.)*")/g)) {
      const key = JSON.parse(match[1]);
      if (!key || key === 'Syncthing') continue;
      assert.ok(Object.hasOwn(translations.nb, key), `${file}: ${key}`);
    }
  }
});

test('localized summary translates counts without translating folder names', () => {
  const panel = load('models/PanelModel.js');
  const tr = (key, args) => translations.text(key, 'nb', args);
  const tn = (key, count, args) => translations.plural(key, count, 'nb', args);
  const ready = {online: true, serviceActive: true};
  // The header keeps no line the tabs or rows already show: no folder count
  // under "Up to date", no paused count under "Some folders paused".
  for (const rows of [[{state: 'idle'}], [{state: 'idle'}, {state: 'idle'}]]) {
    const summary = panel.compactSummary(ready, rows, tr, tn);
    assert.deepEqual([summary.title, summary.detail], ['Up to date', '']);
  }
  const paused = panel.compactSummary(ready, [{state: 'idle', paused: true}, {state: 'idle'}], tr, tn);
  assert.deepEqual([paused.title, paused.detail], ['Some folders paused', '']);
  const missing = panel.compactSummary({online: false, installationState: 'missing', summaryText: 'Syncthing is not installed'}, [], tr, tn);
  assert.deepEqual([missing.title, missing.detail], ['Syncthing is not installed', '']);
  // Starting up still says what it waits for.
  assert.equal(panel.compactSummary({online: false, installationState: 'existing', summaryText: 'Finding Syncthing'}, [], tr, tn).detail, 'Waiting for the local service');
  assert.equal(panel.compactSummary(ready, [{state: 'syncing', label: 'Settings', syncing: true, needBytes: 1024, globalBytes: 2048}], tr, tn).detail, 'Settings · 1.0 KiB gjenstår');
  assert.equal(panel.compactSummary({...ready, hasConflicts: true}, [{state: 'idle', conflict: true}], tr, tn).detail, 'Synkroniseringskonflikter i 1 mappe');
  // "Installation needs cleanup" names the file in the way, literally.
  const unit = '/home/me/.config/systemd/user/syncthing.service';
  const blocked = {online: false, installationState: 'incomplete', installationBlocker: unit, summaryText: 'Installation needs cleanup'};
  const summary = panel.compactSummary(blocked, [], tr, tn);
  assert.equal(summary.title, 'Installation needs cleanup');
  assert.equal(summary.detail, `${unit} finnes, men Syncthing er ikke installert`);
  const odd = '/home/{path} %1/syncthing';
  assert.equal(panel.compactSummary({...blocked, installationBlocker: odd}, [], tr, tn).detail.startsWith(odd + ' finnes'), true);
  assert.equal(panel.compactSummary({...blocked, installationBlocker: ''}, [], tr, tn).detail, 'Waiting for the local service');
  assert.equal(panel.compactSummary({...blocked, installationState: 'missing'}, [], tr, tn).detail, '');
  // Decimals follow the language once the service registers FoamyI18n.number.
  panel.setNumberFormat((value, decimals) => Number(value).toFixed(decimals).replace('.', ','));
  assert.equal(panel.formatBytes(1536), '1,5 KiB');
});
