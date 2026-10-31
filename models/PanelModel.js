.pragma library

// Decimal numbers follow the Omarchy language (FoamyI18n.number) once the
// service registers it; plain toFixed in tests and before that.
var formatNumber = function(value, decimals) { return Number(value).toFixed(decimals) }
function setNumberFormat(fn) { if (typeof fn === "function") formatNumber = fn }

function resolveFolderPath(value, homePath) {
  var path = String(value || "")
  if (path === "~") return homePath
  if (path.indexOf("~/") === 0) return homePath + path.slice(1)
  if (path.charAt(0) === "/" || !homePath) return path
  return homePath + "/" + path
}

function pathLabel(path) {
  var value = String(path || "").replace(/\/+$/, "")
  if (String(path || "").charAt(0) === "/" && value === "") return "/"
  var parts = value.split("/")
  return parts.length > 0 && parts[parts.length - 1]
    ? parts[parts.length - 1] : "Folder"
}

function buildFolderRows(syncthing, homePath) {
  var rows = []
  var source = syncthing && syncthing.folders ? syncthing.folders : []
  var statuses = syncthing && syncthing.folderStatuses
    ? syncthing.folderStatuses : ({})
  var conflicts = syncthing && syncthing.conflictFolderIds
    ? syncthing.conflictFolderIds : ({})
  for (var i = 0; i < source.length; i++) {
    var folder = source[i] || ({})
    var id = String(folder.id || "")
    var status = statuses[id] || ({})
    var state = String(status.state || "unknown")
    var errors = Number(status.errors || 0) + Number(status.pullErrors || 0)
    var needItems = Number(status.needTotalItems || 0)
    // Match Syncthing's idle backlog state; it may require manual resolution.
    var outOfSync = !folder.paused && state === "idle" && needItems > 0
    var configuredLabel = String(folder.label || "")
    var folderDevices = folder.devices || []
    var sharedDeviceCount = 0
    for (var j = 0; j < folderDevices.length; j++) {
      var deviceId = String((folderDevices[j] || {}).deviceID || "")
      if (deviceId && (!syncthing || deviceId !== syncthing.localDeviceId)) {
        sharedDeviceCount++
      }
    }
    var pathName = folder.path ? pathLabel(resolveFolderPath(folder.path, homePath)) : ""
    rows.push({
      id: id,
      // Syncthing's own label first, as in its Web UI; two "Docs" folders in
      // different places stay distinguishable by their path line.
      label: configuredLabel || pathName || id || "Unnamed folder",
      configuredLabel: configuredLabel,
      pathName: pathName,
      markerName: String(folder.markerName || ".stfolder"),
      path: String(folder.path || ""),
      state: state,
      error: String(status.error || ""),
      outOfSync: outOfSync,
      problem: outOfSync || state === "error" || !!status.error || errors > 0,
      // Holds *.sync-conflict-* files (see scripts/syncthing-conflicts.py).
      conflict: conflicts[id] === true,
      // Preparing without queued work is not active synchronization.
      syncing: !outOfSync && (needItems > 0 || state === "syncing"),
      scanning: state.indexOf("scan") === 0,
      paused: !!folder.paused,
      devices: folderDevices,
      errorDetails: status.errorDetails || [],
      sharedDeviceCount: sharedDeviceCount,
      needItems: needItems,
      needBytes: Number(status.needBytes || 0),
      globalFiles: Number(status.globalFiles || 0),
      globalBytes: Number(status.globalBytes || 0)
    })
  }
  return rows
}

function total(rows, key) {
  var value = 0
  for (var i = 0; i < rows.length; i++) value += Number(rows[i][key] || 0)
  return value
}

function stateCount(rows, key) {
  var count = 0
  for (var i = 0; i < rows.length; i++) {
    if (rows[i][key]) count++
  }
  return count
}

function formatCount(value) {
  var count = Math.max(0, Number(value || 0))
  if (count >= 1000000) return formatNumber(count / 1000000, 1) + "m"
  if (count >= 1000) return formatNumber(count / 1000, count >= 10000 ? 0 : 1) + "k"
  return String(Math.round(count))
}

function formatBytes(value) {
  var bytes = Math.max(0, Number(value || 0))
  var units = ["B", "KiB", "MiB", "GiB", "TiB"]
  var unit = 0
  while (bytes >= 1024 && unit < units.length - 1) {
    bytes /= 1024
    unit++
  }
  return (unit === 0 ? String(Math.round(bytes))
    : formatNumber(bytes, bytes >= 10 ? 0 : 1)) + " " + units[unit]
}

function folderMeta(folder, translate, translatePlural) {
  var tr = translate || function(key, values) { return key.replace(/\{(\w+)\}/g, function(m, k) { return values && values[k] !== undefined ? values[k] : m }) }
  var tn = translatePlural || function(key, count, values) { return tr(key, values) }
  if (folder.outOfSync) return tr("Out of sync")
  if (folder.problem) return folder.error || tr("Needs attention")
  if (folder.conflict) return tr("Sync conflicts")
  if (folder.paused) return tr("Syncing paused")
  if (folder.scanning) return tr("Checking local changes")
  if (folder.syncing) {
    var remaining = tn("{count} items remaining", folder.needItems, {count: formatCount(folder.needItems)})
    return remaining + (folder.needBytes > 0 ? " · " + formatBytes(folder.needBytes) : "")
  }
  return tr("{count} files · {size}", {count: formatCount(folder.globalFiles),
    size: folder.sharedDeviceCount === 0 ? tr("Local only") : formatBytes(folder.globalBytes)})
}

function folderState(folder, recentlyLinkedFolderId, hasActivity) {
  if (!folder) return "UNKNOWN"
  if (folder.paused) return "PAUSED"
  if (folder.outOfSync) return "OUT OF SYNC"
  if (folder.problem) return "ERROR"
  if (folder.syncing || folder.scanning || hasActivity) return "SYNCING"
  return "SYNCED"
}

function folderById(rows, folderId) {
  for (var i = 0; i < rows.length; i++) {
    if (rows[i].id === folderId) return rows[i]
  }
  return null
}

function deviceName(devices, deviceId, translate) {
  var tr = translate || function(key, values) { return key.replace(/\{(\w+)\}/g, function(m, k) { return values && values[k] !== undefined ? values[k] : m }) }
  for (var i = 0; i < devices.length; i++) {
    var device = devices[i] || ({})
    if (String(device.deviceID || "") === String(deviceId || "")) {
      return String(device.name || tr("Device {id}", {id: String(device.deviceID || "").slice(0, 7)}))
    }
  }
  return tr("Unknown device")
}

function deviceOptions(syncthing, translate) {
  var tr = translate || function(key, values) { return key.replace(/\{(\w+)\}/g, function(m, k) { return values && values[k] !== undefined ? values[k] : m }) }
  var options = []
  var devices = syncthing && syncthing.devices ? syncthing.devices : []
  for (var i = 0; i < devices.length; i++) {
    var device = devices[i] || ({})
    var id = String(device.deviceID || "")
    if (!id || id === syncthing.localDeviceId) continue
    options.push({
      value: id,
      label: String(device.name || tr("Device {id}", {id: id.slice(0, 7)})),
      encrypted: device.untrusted === true,
      description: device.untrusted
        ? tr("Encrypted sharing requires the Web UI")
        : tr("Will receive a folder share offer")
    })
  }
  return options
}

function pendingOfferOptions(syncthing, translate) {
  var tr = translate || function(key, values) { return key.replace(/\{(\w+)\}/g, function(m, k) { return values && values[k] !== undefined ? values[k] : m }) }
  var options = []
  var pending = syncthing && syncthing.pendingFolders
    ? syncthing.pendingFolders : ({})
  var ids = Object.keys(pending)
  var devices = syncthing && syncthing.devices ? syncthing.devices : []
  for (var i = 0; i < ids.length; i++) {
    var offeredBy = (pending[ids[i]] || {}).offeredBy || ({})
    var deviceIds = Object.keys(offeredBy)
    var encrypted = false
    for (var j = 0; j < deviceIds.length; j++) {
      var candidate = offeredBy[deviceIds[j]] || ({})
      if (candidate.receiveEncrypted === true
          || candidate.remoteEncrypted === true) encrypted = true
    }
    if (encrypted) continue
    for (var offerIndex = 0; offerIndex < deviceIds.length; offerIndex++) {
      var offer = offeredBy[deviceIds[offerIndex]] || ({})
      options.push({
        value: JSON.stringify([ids[i], deviceIds[offerIndex]]),
        folderId: ids[i],
        deviceId: deviceIds[offerIndex],
        folderLabel: String(offer.label || ids[i]),
        label: tr("{folder} from {device}", {folder: String(offer.label || ids[i]),
          device: deviceName(devices, deviceIds[offerIndex], tr)})
      })
    }
  }
  return options
}

function encryptedPendingOfferCount(syncthing) {
  var count = 0
  var pending = syncthing && syncthing.pendingFolders
    ? syncthing.pendingFolders : ({})
  var ids = Object.keys(pending)
  for (var i = 0; i < ids.length; i++) {
    var offeredBy = (pending[ids[i]] || {}).offeredBy || ({})
    var deviceIds = Object.keys(offeredBy)
    var encrypted = false
    for (var j = 0; j < deviceIds.length; j++) {
      var offer = offeredBy[deviceIds[j]] || ({})
      if (offer.receiveEncrypted === true || offer.remoteEncrypted === true) {
        encrypted = true
      }
    }
    if (encrypted) count++
  }
  return count
}

// Preserve the established device totals while the core reports remote peers.
function deviceCounts(service) {
  if (!service) return { connected: 0, total: 0 }
  var local = service.localDeviceId ? 1 : 0
  return {
    connected: Number(service.connectedDeviceCount || 0) + local,
    total: Number(service.deviceCount || 0) + local
  }
}

function compactFolderState(folder, online, stopped, hasActivity) {
  if (stopped) return "Stopped"
  if (!online) return "Unavailable"
  if (folder.outOfSync) return "Out of sync"
  if (folder.problem) return "Needs attention"
  if (folder.conflict) return "Sync conflicts"
  if (folder.paused) return "Paused"
  if (folder.scanning) return "Scanning"
  if (folder.syncing || hasActivity) return "Syncing"
  if (folder.state !== "idle") return "Checking"
  return "Up to date"
}

function compactSummary(service, rows, translate, translatePlural) {
  var tr = translate || function(key, values) { return key.replace(/\{(\w+)\}/g, function(m, k) { return values && values[k] !== undefined ? values[k] : m }) }
  var tn = translatePlural || function(key, count, values) { return tr(key, values || {count: count}) }
  var result = { title: "Syncthing unavailable", detail: "Waiting for the local service", progress: -1 }
  if (!service) return result
  if (service.serviceAvailable && !service.serviceActive) {
    result.title = "Syncing stopped"
    result.detail = "Start syncing to reconnect"
    return result
  }
  if (!service.online) {
    result.title = service.summaryText || result.title
    return result
  }
  if (service.hasConflicts || stateCount(rows, "problem") > 0) {
    var outOfSync = !service.hasConflicts && stateCount(rows, "outOfSync") > 0
    var conflicted = stateCount(rows, "conflict")
    result.title = outOfSync ? "Out of sync" : "Needs attention"
    result.detail = service.hasConflicts ? (conflicted > 0
        ? tn("Sync conflicts in {count} folders", conflicted, {count: conflicted}) : "Sync conflicts detected")
      : outOfSync ? "Check the affected folders in the Web UI" : "Check the folder errors below"
    return result
  }
  if (!rows.length) {
    result.title = "No shared folders"
    result.detail = "Add a folder in settings"
    return result
  }
  var active = rows.filter(function(row) { return !row.paused && row.syncing })
  if (active.length) {
    var remaining = total(active, "needBytes")
    var bytes = total(active, "globalBytes")
    result.title = "Syncing files"
    result.detail = tr("{folder} · {amount} remaining", {folder: active.length === 1 ? active[0].label : tn("{count} folders", active.length, {count: active.length}),
      amount: remaining > 0 ? formatBytes(remaining) : tn("{count} items", total(active, "needItems"), {count: formatCount(total(active, "needItems"))})})
    // Metadata/deletion work has no byte denominator. Never report completion
    // while queued work remains, even when rounding would otherwise show 100%.
    if (bytes > 0 && remaining > 0)
      result.progress = Math.min(99, Math.max(0, Math.floor(100 * (bytes - remaining) / bytes)))
    return result
  }
  if (stateCount(rows, "scanning") > 0) {
    result.title = "Scanning folders"
    result.detail = "Checking local changes"
    return result
  }
  if (service.syncActivityDetail) {
    result.title = "Syncing files"
    result.detail = service.syncActivityDetail
    return result
  }
  var paused = stateCount(rows, "paused")
  if (paused > 0) {
    result.title = paused === rows.length ? "Syncing paused" : "Some folders paused"
    result.detail = tr("{paused} of {total} folders paused", {paused: paused, total: rows.length})
    return result
  }
  if (rows.some(function(row) { return row.state !== "idle" })) {
    result.title = "Checking folders"
    result.detail = "Waiting for folder status"
    return result
  }
  result.title = "Up to date"
  result.detail = tn("{count} shared folders", rows.length, {count: rows.length})
  result.progress = 100
  return result
}
