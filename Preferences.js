function integer(value, fallback, min, max) {
  return typeof value === "number" && isFinite(value) && Math.floor(value) === value
    && value >= min && value <= max ? value : fallback
}
function fromSettings(value) {
  value = value && typeof value === "object" && !Array.isArray(value) ? value : {}
  return {
    minimumConnectedDevices: integer(value.minimumConnectedDevices, 0, 0, 100),
    refreshIntervalSec: integer(value.refreshIntervalSec, 60, 60, 3600),
    openRefreshIntervalSec: integer(value.openRefreshIntervalSec, 5, 2, 60),
    probeIntervalSec: integer(value.probeIntervalSec, 15, 5, 300),
    serviceState: value.serviceState === "disabled" ? "disabled" : "enabled"
  }
}
function widgetSettings(config, id) {
  var layout = config && config.bar && config.bar.layout || {}
  for (var section of ["left", "centerBegin", "center", "centerEnd", "tray", "right"]) {
    var entries = Array.isArray(layout[section]) ? layout[section] : []
    for (var entry of entries) if (entry && entry.id === id) return entry
  }
  return {}
}
