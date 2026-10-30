// Exercise the real kit FoamyI18n substitution and plural code with catalog data in pure model tests.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../kit/FoamyI18n.qml'), 'utf8');
const start = source.indexOf('  function englishText(');
const end = source.indexOf('\n  function number', start);
const scope = vm.createContext({backend: null, english: {}, root: {number: v => String(v)}});
vm.runInContext(source.slice(start, end), scope);
const en = require('../i18n/en.json');
const no = require('../i18n/no.json');
function pick(value, count) {
  if (value && typeof value === 'object') return count !== undefined && Math.abs(count) === 1 ? value.one : value.other;
  return value;
}
function backendFor(language) {
  const catalog = language === 'no' || language === 'nb' ? no : en;
  return {
    t(label, fallback) { const v = pick(catalog[label]); return typeof v === 'string' ? v : fallback; },
    tn(label, count, fallback) { const v = pick(catalog[label], count) ?? pick(en[label], count); return typeof v === 'string' ? v : fallback; },
  };
}
function text(key, language, values) { scope.backend = backendFor(language); return scope.t(key, values); }
function plural(key, count, language, values) { scope.backend = backendFor(language); return scope.tn(key, count, values); }
module.exports = {text, plural, no, en, norwegian: no, nb: no};
