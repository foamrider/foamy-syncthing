const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');
function load(name) {
  const scope = vm.createContext({});
  vm.runInContext(fs.readFileSync(path.join(__dirname, '..', name), 'utf8').replace(/^\.pragma library\s*/, ''), scope);
  return scope;
}
const prefs = load('Preferences.js');
const translations = load('Translations.js');

test('system language and explicit overrides follow the weather plugin', () => {
  for (const locale of ['nb_NO', 'nn-NO', 'no']) assert.equal(prefs.fromSettings({}, locale).language, 'nb');
  for (const locale of ['en_US', 'de_DE', 'not-a-locale', '']) assert.equal(prefs.fromSettings({}, locale).language, 'en');
  assert.equal(prefs.fromSettings({language:'en'}, 'nb_NO').language, 'en');
  assert.equal(prefs.fromSettings({language:'nb'}, 'en_US').language, 'nb');
  assert.equal(prefs.fromSettings({language:'xx'}, 'en_US').languageMode, 'system');
});

test('public defaults and bounds do not inherit personal warning settings', () => {
  assert.equal(prefs.fromSettings({}, 'en').minimumConnectedDevices, 0);
  for (const value of [-1, 101, 2.5, '3', null]) assert.equal(prefs.fromSettings({minimumConnectedDevices:value}, 'en').minimumConnectedDevices, 0);
  assert.equal(prefs.fromSettings({minimumConnectedDevices:3}, 'en').minimumConnectedDevices, 3);
  assert.equal(prefs.fromSettings({openRefreshIntervalSec:1}, 'en').openRefreshIntervalSec, 5);
  assert.equal(prefs.fromSettings({refreshIntervalSec:30}, 'en').refreshIntervalSec, 60);
  assert.equal(prefs.fromSettings({refreshIntervalSec:120}, 'en').refreshIntervalSec, 120);
  assert.equal(prefs.fromSettings({serviceState:'unexpected'}, 'en').serviceState, 'enabled');
});

test('settings lookup uses only the requested installed namespace', () => {
  const config = {bar:{layout:{left:[{id:'foamy.weather',language:'en'}],right:[{id:'foamy.syncthing',language:'nb'}]}}};
  assert.equal(prefs.widgetSettings(config, 'foamy.syncthing').language, 'nb');
  assert.equal(Object.keys(prefs.widgetSettings(config, 'other.syncthing')).length, 0);
});

test('translations preserve placeholders and literal user data', () => {
  const placeholders = value => [...value.matchAll(/\{(\w+)\}/g)].map(match => match[1]).sort();
  for (const [key, value] of Object.entries(translations.nb)) assert.deepEqual(placeholders(value), placeholders(key), key);
  const name = 'Settings {name} $& <folder>';
  assert.equal(translations.text('Unlink {name} from Syncthing; keep local files', 'nb', {name}),
    `Koble ${name} fra Syncthing; behold lokale filer`);
  assert.equal(translations.text('Settings', 'en'), 'Settings');
  assert.equal(translations.text('Unknown backend error', 'nb'), 'Unknown backend error');
});

test('every static Foamy UI translation key has Norwegian text', () => {
  const files = ['Panel.qml','Service.qml','ui/SyncthingPanelPopup.qml','ui/SyncthingOverview.qml','models/OverviewModel.js','ui/SyncthingSettings.qml','ui/AddFolderForm.qml'];
  for (const file of files) {
    const source = fs.readFileSync(path.join(__dirname, '..', file), 'utf8');
    for (const match of source.matchAll(/\btr\(("(?:[^"\\]|\\.)*")/g)) {
      const key = JSON.parse(match[1]);
      if (!key || key === 'Syncthing') continue;
      assert.ok(Object.hasOwn(translations.nb, key), `${file}: ${key}`);
    }
  }
});

test('localized summary translates counts without translating folder names', () => {
  const panel = load('models/PanelModel.js');
  const tr = (key, args) => translations.text(key, 'nb', args);
  const ready = {online:true,serviceActive:true};
  assert.equal(panel.compactSummary(ready,[{state:'idle'}],tr).detail,'1 delt mappe');
  assert.equal(panel.compactSummary(ready,[{state:'idle',paused:true}],tr).detail,'1 av 1 mapper er satt på pause');
  assert.equal(panel.compactSummary(ready,[{state:'syncing',label:'Settings',syncing:true,needBytes:1024,globalBytes:2048}],tr).detail,'Settings · 1.0 KiB gjenstår');
});
