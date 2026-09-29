.pragma library

function folderState(folder, service, peers) {
  if (!service || !service.online) return "Unavailable"
  if (folder.problem) return "Error"
  if (folder.paused) return "Paused"
  if (folder.scanning) return "Scanning"
  if (folder.state === "syncing" || (service.syncActivityFolderId === folder.id && service.syncActivityDetail)) return "Syncing"
  if (peers.length && !peers.some(function(peer) { return peer.connected })) return "Disconnected"
  if (folder.needItems > 0) return "Waiting to sync"
  if (folder.state !== "idle") return "Checking"
  return "Up to date"
}

function errorText(folder) {
  var lines = folder.error ? [folder.error] : []
  var errors = folder.errorDetails || []
  for (var i = 0; i < errors.length; i++) {
    var value = errors[i]
    var message = typeof value === "string" ? value
      : value && value.error ? (value.path ? value.path + ": " : "") + value.error : ""
    if (message && lines.indexOf(message) < 0) lines.push(message)
  }
  return lines.join("\n")
}

function rows(service, folders, translate, formatBytes, formatCount) {
  var tr = translate
  var devices = service && service.devices ? service.devices : []
  var byId = Object.create(null)
  var deviceRows = []
  var folderRows = []
  // Index memberships once per snapshot; both tabs share the same relationships.
  for (var i = 0; i < devices.length; i++) {
    var device = devices[i]
    var id = String(device.deviceID || "")
    if (!id || id === service.localDeviceId || byId[id]) continue
    var row = {id: id, label: device.name || tr("Device {id}", {id: id.slice(0, 7)}),
      connected: device.connected === true, state: "", subtitle: "", detail: "", related: [], icon: "devices", progress: -1}
    byId[id] = row
    deviceRows.push(row)
  }
  for (var j = 0; j < folders.length; j++) {
    var folder = folders[j]
    var members = folder.devices || []
    var peers = []
    var seen = Object.create(null)
    for (var k = 0; k < members.length; k++) {
      var memberId = String(members[k].deviceID || "")
      if (!memberId || memberId === (service && service.localDeviceId) || seen[memberId]) continue
      seen[memberId] = true
      // Missing devices can be a truncated snapshot, not a disconnected peer.
      peers.push(byId[memberId] || {id: memberId, label: tr("Device {id}", {id: memberId.slice(0, 7)}), connected: false, missing: true})
    }
    var state = folderState(folder, service, peers)
    if (state === "Disconnected" && peers.some(function(peer) { return peer.missing })) state = "Unavailable"
    var detail = folder.path
    if (state === "Error") detail += "\n" + (errorText(folder) || tr("Check the folder errors in the Web UI"))
    if (state === "Unavailable") detail += "\n" + tr("Current status is unavailable.")
    if (!peers.length) detail += "\n" + tr("Local only")
    if (state === "Syncing" || state === "Waiting to sync") {
      detail += "\n" + tr("{count} items remaining", {count: formatCount(folder.needItems)})
      if (service.syncActivityFolderId === folder.id && service.syncActivityDetail) detail += "\n" + service.syncActivityDetail
    }
    var progress = -1
    // Queued bytes describe local folder completion, never a peer's transfer rate.
    if (state === "Syncing" && folder.globalBytes > 0 && folder.needBytes > 0)
      progress = Math.max(0, Math.min(99, Math.floor(100 * (folder.globalBytes - folder.needBytes) / folder.globalBytes)))
    var entry = {id: folder.id, label: folder.label, state: state, icon: "folder", folder: folder,
      subtitle: tr("{count} files · {size}", {count: formatCount(folder.globalFiles), size: formatBytes(folder.globalBytes)}),
      detail: detail, progress: progress, related: []}
    peers.forEach(function(peer) {
      entry.related.push({label: peer.label, state: !service || !service.online || peer.missing ? "Unavailable" : peer.connected ? "Connected" : "Disconnected"})
      if (!peer.missing) peer.related.push({label: folder.label, state: state})
    })
    folderRows.push(entry)
  }
  deviceRows.forEach(function(row) {
    row.state = !service || !service.online ? "Unavailable" : !row.connected ? "Disconnected" : "Connected"
    if (row.state === "Connected") {
      if (row.related.some(function(folder) { return folder.state === "Error" })) row.state = "Error"
      else if (row.related.some(function(folder) { return folder.state === "Syncing" })) row.state = "Syncing"
    }
    row.subtitle = tr(row.related.length === 1 ? "{count} shared folder" : "{count} shared folders", {count: row.related.length})
    row.detail = row.state === "Error" ? tr("Connected. A shared folder has a local error.")
      : row.state === "Syncing" ? tr("Connected. A shared folder is syncing locally.")
      : row.state === "Unavailable" ? tr("Current status is unavailable.") : ""
    if (!row.related.length) row.detail += (row.detail ? "\n" : "") + tr("No shared folders")
  })
  return {folders: folderRows, devices: deviceRows}
}
