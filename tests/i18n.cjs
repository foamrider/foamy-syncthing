// The kit's FoamyI18n substitution and plural rules (kit/FoamyI18nText.js) over
// this plugin's catalogs, for pure model tests.
const I18n = require('../kit/node-loader.cjs').load('FoamyI18nText');
const en = require('../i18n/en.json');
const no = require('../i18n/no.json');
const number = (v, d) => (d === undefined ? String(v) : Number(v).toFixed(d));
const scope = language => I18n.translator(language === 'no' || language === 'nb' ? no : en, en, number);
function text(key, language, values) { return scope(language).t(key, values); }
function plural(key, count, language, values) { return scope(language).tn(key, count, values); }
module.exports = {text, plural, no, en, norwegian: no, nb: no};
