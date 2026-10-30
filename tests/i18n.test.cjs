const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const I = require('./i18n.cjs');
const manifest = require('../manifest.json');
const tokens = s => [...s.matchAll(/%[1-9][0-9]*|\{\w+\}/g)].map(m => m[0]).sort();
for (const [key, value] of Object.entries(I.en)) {
  const forms = typeof value === 'object' ? Object.values(value) : [value];
  if (typeof value === 'string') assert.equal(value, key, `English source key: ${key}`);
  if (Object.hasOwn(I.no, key)) {
    const no = typeof I.no[key] === 'object' ? Object.values(I.no[key]) : [I.no[key]];
    for (const form of no) assert.deepEqual(tokens(form), tokens(forms[forms.length - 1]), key);
  }
}
assert.equal(manifest.barWidget?.defaults?.language, undefined);
assert.equal((manifest.barWidget?.schema || []).some(f => f.key === 'language'), false);
assert.equal(I.text('Unknown %1 {name}', 'no', ['$& %2']), '$& %2 {name}'.replace(/^/, 'Unknown '));
assert.equal(I.text('constructor', 'no'), 'constructor');
const ui = fs.readFileSync(path.join(__dirname, '../kit/FoamyI18n.qml'), 'utf8');
assert.ok(ui.includes('Qt.createQmlObject'), '4.0 must not statically require a 4.5 QML type');
