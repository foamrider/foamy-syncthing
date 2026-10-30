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
const prefs = load('kit/FoamyPrefs.js');
const manifest = require('../manifest.json');
const translations = require('./i18n.cjs');
const value = (settings, key) => prefs.value(schema.fields, settings, key);

test('public defaults and bounds do not inherit personal warning settings', () => {
  assert.equal(value({}, 'minimumConnectedDevices'), 0);
  for (const bad of [-1, 101, 2.5, '3', null]) assert.equal(value({minimumConnectedDevices: bad}, 'minimumConnectedDevices'), 0);
  assert.equal(value({minimumConnectedDevices: 3}, 'minimumConnectedDevices'), 3);
  assert.equal(value({openRefreshIntervalSec: 1}, 'openRefreshIntervalSec'), 5);
  assert.equal(value({refreshIntervalSec: 30}, 'refreshIntervalSec'), 60);
  assert.equal(value({refreshIntervalSec: 120}, 'refreshIntervalSec'), 120);
  assert.equal(value({probeIntervalSec: 301}, 'probeIntervalSec'), 15);
  assert.equal(value({serviceState: 'unexpected'}, 'serviceState'), 'enabled');
  assert.equal(value({serviceState: 'disabled'}, 'serviceState'), 'disabled');
});

test('the schema mirrors the manifest defaults and schema', () => {
  const defaults = manifest.barWidget.defaults;
  for (const field of schema.fields) assert.equal(defaults[field.key], field.defaultValue, field.key);
  assert.deepEqual(Object.keys(defaults).sort(), schema.fields.map(f => f.key).sort());
  for (const entry of manifest.barWidget.schema || []) {
    const field = schema.fields.find(f => f.key === entry.key);
    assert.ok(field, entry.key);
    for (const key of ['type', 'min', 'max', 'defaultValue']) assert.equal(entry[key], field[key], `${entry.key}.${key}`);
  }
});

test('saving validates and uses the full entry, or the omarchy bar set fallback', () => {
  const calls = [];
  const bar = {shell: {updateEntryInline(id, entry) { calls.push([id, entry]); return true; }}};
  const settings = {id: 'foamy.syncthing', minimumConnectedDevices: 0, custom: 'kept'};
  const ok = prefs.save(bar, 'foamy.syncthing', schema.fields, settings, 'minimumConnectedDevices', 2);
  assert.equal(ok.ok, true);
  assert.equal(ok.inline, true);
  assert.equal(JSON.stringify(calls[0]), JSON.stringify(['foamy.syncthing', {minimumConnectedDevices: 2, custom: 'kept'}]));
  assert.equal(prefs.save(bar, 'foamy.syncthing', schema.fields, settings, 'minimumConnectedDevices', 101).ok, false);
  const cli = prefs.save(null, 'foamy.syncthing', schema.fields, settings, 'refreshIntervalSec', 300);
  assert.equal(JSON.stringify(cli.command), JSON.stringify(['omarchy', 'bar', 'set', 'foamy.syncthing', 'refreshIntervalSec', '300', '--json']));
});

test('translations preserve placeholders and literal user data', () => {
  const placeholders = v => [...v.matchAll(/\{(\w+)\}|%(\d)/g)].map(match => match[0]).sort();
  for (const [key, v] of Object.entries(translations.nb)) {
    if (typeof v === 'string') assert.deepEqual(placeholders(v), placeholders(key), key);
  }
  const name = 'Settings {name} $& <folder>';
  assert.equal(translations.text('Paused {name}. Folder configuration, sharing, and local files are unchanged.', 'nb', {name}).includes(name), true);
  assert.equal(translations.text('Settings', 'en'), 'Settings');
  assert.equal(translations.text('Unknown backend error', 'nb'), 'Unknown backend error');
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
  const ready = {online: true, serviceActive: true};
  assert.equal(panel.compactSummary(ready, [{state: 'idle'}], tr).detail, '1 delt mappe');
  assert.equal(panel.compactSummary(ready, [{state: 'idle', paused: true}], tr).detail, '1 av 1 mapper er satt på pause');
  assert.equal(panel.compactSummary(ready, [{state: 'syncing', label: 'Settings', syncing: true, needBytes: 1024, globalBytes: 2048}], tr).detail, 'Settings · 1.0 KiB gjenstår');
});
