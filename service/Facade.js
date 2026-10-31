.pragma library

function devices(values) {
  var projected = []
  var source = values || []
  for (var i = 0; i < source.length; i++) {
    var device = source[i] || ({})
    projected.push({
      deviceID: String(device.id || ""),
      name: String(device.name || ""),
      untrusted: device.untrusted === true,
      connected: device.connected === true
    })
  }
  return projected
}

function folderStatus(value) {
  var status = value || ({})
  return {
    state: String(status.state || "unknown"),
    error: String(status.error || ""),
    errors: (status.errors || []).length,
    errorDetails: status.errors || [],
    pullErrors: Number(status.pullErrors || 0),
    needTotalItems: Number(status.needTotalItems || 0),
    needBytes: Number(status.needBytes || 0),
    globalFiles: Number(status.globalFiles || 0),
    globalBytes: Number(status.globalBytes || 0)
  }
}

function folders(values) {
  var projected = []
  var source = values || []
  for (var i = 0; i < source.length; i++) {
    var folder = source[i] || ({})
    var folderDevices = []
    var shared = folder.devices || []
    for (var j = 0; j < shared.length; j++) {
      folderDevices.push({ deviceID: String((shared[j] || {}).id || "") })
    }
    projected.push({
      id: String(folder.id || ""),
      label: String(folder.label || ""),
      path: String(folder.path || ""),
      paused: folder.paused === true,
      markerName: String(folder.markerName || ".stfolder"),
      devices: folderDevices
    })
  }
  return projected
}

function folderStatuses(values) {
  var projected = ({})
  var source = values || []
  for (var i = 0; i < source.length; i++) {
    var folder = source[i] || ({})
    projected[String(folder.id || "")] = folderStatus(folder.status)
  }
  return projected
}

function syncingFiles(activity) {
  var projected = []
  var files = activity && activity.files ? activity.files : []
  for (var i = 0; i < files.length; i++) {
    projected.push(String((files[i] || {}).detail || ""))
  }
  return projected
}

function lifecyclePresentation(lifecycle) {
  var state = lifecycle || ({})
  var controllable = state.canControl === true || state.canStart === true
  return {
    available: state.targetMatch === true && controllable,
    controllable: controllable
  }
}

// True when the core truncated a list to its panel limits.
function truncated(truncation) {
  var state = truncation || ({})
  var keys = ["devices", "folders", "folderDevices",
    "pendingFolders", "pendingOffers", "pendingDevices", "nearbyDevices"]
  for (var i = 0; i < keys.length; i++) {
    if (Number(state[keys[i]] || 0) > 0) return true
  }
  return false
}
