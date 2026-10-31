// Widget settings schema (mirrors manifest.json barWidget.defaults and
// schema). Values are read and saved through kit/FoamyPrefs.js.
var fields = [
  { key: "minimumConnectedDevices", type: "integer", min: 0, max: 100, step: 1, defaultValue: 0 },
  { key: "refreshIntervalSec", type: "integer", min: 60, max: 3600, step: 60, defaultValue: 60 },
  { key: "openRefreshIntervalSec", type: "integer", min: 2, max: 60, step: 1, defaultValue: 5 },
  { key: "probeIntervalSec", type: "integer", min: 5, max: 300, step: 5, defaultValue: 15 },
  { key: "serviceState", type: "enum", options: ["enabled", "disabled"], defaultValue: "enabled" }
]
if (typeof module !== "undefined") module.exports = { fields: fields }
