import QtQuick
import Quickshell
import Quickshell.Io
import "." as Plugin
import "kit"
import "kit/FoamyPrefs.js" as Prefs
import "Preferences.js" as Schema
import "service"
import "service/Facade.js" as Facade
import "models/PanelModel.js" as PanelModel

// The plugin's service entry point: the shell creates one per shell (not per
// bar). It owns the bundled native core, the install probe and the conflict
// scanner, and shares itself with the bar widgets through ServiceRegistry.
// The bar widget leader hands it the widget settings (widgetSettings).
QtObject {
  id: root

  function tr(key, values) { return FoamyI18n.t(key, values) }

  readonly property string pluginRoot: localPath(Qt.resolvedUrl("."))
  readonly property string homePath: Quickshell.env("HOME") || ""

  // ---- settings (fed by the bar widget leader) -------------------------------
  property var widgetSettings: ({})
  property bool settingsReady: false
  readonly property var preferences: Prefs.values(Schema.fields, widgetSettings)
  readonly property string configuredServiceState: preferences.serviceState
  readonly property int probeIntervalSeconds: preferences.probeIntervalSec
  readonly property int refreshIntervalSec: preferences.refreshIntervalSec

  // ---- native core snapshot ---------------------------------------------------
  readonly property var state: core.snapshot || ({})
  readonly property var connection: state.connection || ({})
  readonly property var identity: state.identity || ({})
  readonly property var lifecycle: state.lifecycle || ({})
  readonly property var activity: state.activity || ({})
  readonly property var currentActivity: activity.current || ({})
  readonly property var webUi: state.webUi || ({})
  readonly property var counts: state.counts || ({})
  readonly property var truncation: state.truncation || ({})
  readonly property var lifecyclePresentation: Facade.lifecyclePresentation(lifecycle)

  readonly property string phase: core.starting ? "core-starting"
    : core.incompatible ? "core-incompatible"
    : core.unavailable && !core.running ? "core-unavailable"
    : String(connection.phase || "discovering")
  readonly property bool online: core.protocolReady && connection.online === true
  property bool refreshing: false
  readonly property string lastError: core.lastError
    || String(connection.error && connection.error.message || "")
  readonly property string recoveryWarning: core.starting ? root.tr("Starting the native core")
    : Facade.truncationWarning(truncation) ? root.tr("Some Syncthing items exceed panel limits; use the Web UI for the hidden entries") : ""
  readonly property string baseUrl: String(webUi.url || "")
  readonly property string localDeviceId: String(identity.deviceId || "")
  readonly property var devices: Facade.devices(state.devices)
  readonly property var folders: Facade.folders(state.folders)
  readonly property var pendingFolders: state.pendingFolders || ({})
  readonly property var folderStatuses: Facade.folderStatuses(state.folders)
  readonly property var syncingFiles: Facade.syncingFiles(activity)
  readonly property int folderCount: Number(counts.folders || 0)
  readonly property int deviceCount: Number(counts.devices || 0)
  readonly property int connectedDeviceCount: Number(counts.connectedDevices || 0)
  readonly property int folderProblemCount: Number(counts.folderProblems || 0)
  readonly property int syncingFolderCount: Number(counts.syncingFolders || 0)
  readonly property string summaryText: summary()
  // One projection per snapshot for every bar and popup.
  readonly property var folderRows: PanelModel.buildFolderRows(root, homePath)

  readonly property string installationState: online ? "existing" : packageController.state
  readonly property bool canInstall: !online && packageController.canInstall
  readonly property string packageStatus: packageController.packageStatus
  readonly property string packageError: packageController.packageError
  readonly property bool serviceAvailable: lifecyclePresentation.available
  readonly property bool serviceActive: lifecycle.active === true
  property bool serviceActionRunning: false
  readonly property bool canControlService: core.protocolReady && lifecyclePresentation.controllable
  property string controlError: ""

  property bool folderMutationBusy: false
  property string folderMutationId: ""
  property string folderMutationAction: ""
  property string folderMutationError: ""
  property string folderMutationNotice: ""
  property bool folderPreparationBusy: false
  property string folderPreparationError: ""
  property string folderIdSuggestion: ""

  readonly property string syncActivityFolderId: String(currentActivity.folderId || "")
  readonly property string syncActivityAction: String(currentActivity.action || "")
  readonly property string syncActivityDetail: String(currentActivity.detail || "")

  // Folder ids whose directory holds *.sync-conflict-* files ({id: true}).
  property var conflictFolderIds: ({})
  readonly property bool hasConflicts: Object.keys(conflictFolderIds).length > 0
  property string conflictScanError: ""
  property string conflictOutput: ""
  property string conflictInput: ""
  // The folder set the last scan covered; snapshots that keep it skip the walk.
  property string scannedFolderKey: ""
  property var conflictScanFolders: []
  property bool conflictRescan: false
  readonly property string folderKey: folders.map(function(folder) {
    return String(folder.id || "") + "\u0001" + String(folder.path || "")
  }).join("\u0000")

  function localPath(url) {
    var value = String(url || "")
    if (value.indexOf("file://") === 0) value = value.slice(7)
    value = decodeURIComponent(value)
    return value.length > 1 && value.charAt(value.length - 1) === "/" ? value.slice(0, -1) : value
  }

  function summary() {
    if (installationState === "missing") return root.tr("Syncthing is not installed")
    if (installationState === "incomplete") return root.tr("Installation needs cleanup")
    if (serviceAvailable && !serviceActive) return root.tr("Syncing stopped")
    if (phase === "core-starting") return root.tr("Starting the native core")
    if (phase === "discovering") return root.tr("Finding Syncthing")
    if (phase === "loading") return root.tr("Reading status")
    if (phase === "error" || phase === "core-unavailable" || phase === "core-incompatible")
      return root.tr("Syncthing unavailable")
    if (folderProblemCount > 0) return FoamyI18n.tn("%1 folder problems", folderProblemCount)
    if (syncingFolderCount > 0) return FoamyI18n.tn("%1 folders syncing", syncingFolderCount)
    return root.tr("Up to date")
  }

  function refresh(recheckErrors) {
    packageController.updateStatus()
    if (!core.protocolReady || refreshing) return false
    refreshing = true
    if (recheckErrors === true) folderMutationError = ""
    var callback = function(ok, data, error) {
      root.refreshing = false
      if (!ok && recheckErrors === true)
        root.folderMutationError = root.actionError(error, root.tr("Could not recheck Syncthing errors"))
    }
    var request = recheckErrors === true && folderProblemCount > 0
      ? core.action("folder.recheck-errors", {}, callback)
      : core.refresh(callback)
    if (!request) refreshing = false
    return !!request
  }

  function recoverCoreIfNeeded() {
    if (packageController.state !== "existing" || core.running
        || core.protocolReady || core.incompatible || !core.executableReady
        || !core.unavailable || core.everProtocolReady) return false
    return core.restart()
  }

  function configureCore() {
    if (!core.protocolReady || !settingsReady) return
    core.configure({
      probeIntervalSeconds: probeIntervalSeconds,
      refreshIntervalSeconds: refreshIntervalSec,
      desiredServiceState: configuredServiceState
    })
  }

  function actionError(error, fallback) {
    return String(error && error.message || fallback || root.tr("Action failed"))
  }

  function publishFolderNotice(notice) {
    folderMutationNotice = String(notice || "")
    if (!folderMutationNotice) return
    noticeTimer.restart()
  }

  function clearFolderAction() {
    folderMutationBusy = false
    folderMutationAction = ""
    folderMutationId = ""
  }

  function finishFolderAction(contractAction, folderId, notice) {
    clearFolderAction()
    publishFolderNotice(notice)
  }

  function failFolderAction(error, fallback) {
    clearFolderAction()
    folderMutationError = actionError(error, fallback)
  }

  function runFolderAction(action, contractAction, folderId, args, notice) {
    if (!online || folderMutationBusy) {
      folderMutationError = online ? root.tr("Another folder operation is already running")
        : root.tr("Syncthing must be online to manage folders")
      return false
    }
    folderMutationBusy = true
    folderMutationAction = contractAction
    folderMutationId = String(folderId || "")
    folderMutationError = ""
    noticeTimer.stop()
    folderMutationNotice = ""
    var id = core.action(action, args || ({}), function(ok, data, error) {
      if (!ok) {
        if (action === "folder.add-existing" && error && error.code === "path_missing" && !args.createDirectory) {
          root.clearFolderAction()
          // Directory creation needs its own confirmation UI; keep existing directories only.
          root.folderMutationError = root.tr("The directory does not exist. Choose an existing directory.")
          return
        }
        root.failFolderAction(error, root.tr("Could not complete the folder operation"))
        return
      }
      root.finishFolderAction(contractAction, folderId, notice)
    })
    if (id) return true
    failFolderAction(null, root.tr("The native core is not ready"))
    return false
  }

  function configuredFolder(folderId) {
    var wanted = String(folderId || "")
    for (var i = 0; i < folders.length; i++) {
      if (String(folders[i].id || "") === wanted) return folders[i]
    }
    return null
  }

  function folderLabel(folderId) {
    var folder = configuredFolder(folderId)
    return folder ? folder.label || folderId : folderId
  }

  function setFolderPaused(folderId, paused) {
    return runFolderAction(paused ? "folder.pause" : "folder.resume",
      paused ? "pause" : "resume", folderId, { folderId: folderId },
      root.tr(paused ? "Paused {name}. Folder configuration, sharing, and local files are unchanged."
        : "Resumed {name}. Folder configuration, sharing, and local files are unchanged.", {name: folderLabel(folderId)}))
  }

  function unlinkFolder(folderId) {
    if (!online || folderMutationBusy) {
      folderMutationError = online ? root.tr("Another folder operation is already running")
        : root.tr("Syncthing must be online to unlink a folder")
      return false
    }
    var folder = configuredFolder(folderId)
    if (!folder) {
      folderMutationError = root.tr("The selected folder is no longer configured")
      return false
    }
    clearFolderMutationMessage()
    folderMutationBusy = true
    folderMutationAction = "unlink"
    folderMutationId = folderId
    if (folder.paused) return removeUnlinkedFolder(folderId, false)

    // The native removal contract requires pause first. Keep both steps busy
    // so another folder operation cannot run between pause and removal.
    var request = core.action("folder.pause", { folderId: folderId }, function(ok, data, error) {
      if (!ok) {
        root.failFolderAction(error, root.tr("Could not pause the folder before unlinking"))
        return
      }
      root.removeUnlinkedFolder(folderId, true)
    })
    if (!request) failFolderAction(null, root.tr("Could not start unlinking the folder"))
    return !!request
  }

  function removeUnlinkedFolder(folderId, pausedForUnlink) {
    var label = folderLabel(folderId)
    var request = core.action("folder.forget", { folderId: folderId }, function(ok, data, error) {
      if (!ok) {
        root.failFolderAction({ message: root.actionError(error, root.tr("Could not unlink the folder"))
          + (pausedForUnlink ? " " + root.tr("The folder remains paused; its local files are unchanged.") : "") })
        return
      }
      root.finishFolderAction("unlink", folderId,
        root.tr("Unlinked {name} from Syncthing. Local files were kept.", {name: label}))
    })
    if (!request) failFolderAction(null, root.tr("Could not remove the folder from Syncthing.")
      + (pausedForUnlink ? " " + root.tr("The folder remains paused; its local files are unchanged.") : ""))
    return !!request
  }

  function addFolder(path, label, folderId, selectedDeviceIds, pendingDeviceId) {
    var shared = (selectedDeviceIds || []).slice()
    return runFolderAction("folder.add-existing", "add", folderId, {
      folderId: folderId,
      path: path,
      createDirectory: false,
      label: label,
      deviceIds: shared,
      pendingDeviceId: pendingDeviceId
    }, shared.length > 0
      ? root.tr("Selected devices receive a share offer to accept.")
      : root.tr("Local only — this folder will not sync with other devices."))
  }

  function requestFolderIdSuggestion() {
    if (!online || folderPreparationBusy) return
    folderPreparationBusy = true
    folderPreparationError = ""
    folderIdSuggestion = ""
    var id = core.action("folder.suggest-id", {}, function(ok, data, error) {
      root.folderPreparationBusy = false
      if (ok) root.folderIdSuggestion = String(data && data.folderId || "")
      else root.folderPreparationError = root.actionError(error, root.tr("Could not generate a folder ID"))
    })
    if (!id) {
      folderPreparationBusy = false
      folderPreparationError = root.tr("The native core is not ready")
    }
  }

  function clearFolderMutationMessage() {
    noticeTimer.stop()
    folderMutationError = ""
    folderMutationNotice = ""
  }

  function clearFolderMutationNotice() {
    noticeTimer.stop()
    folderMutationNotice = ""
  }

  function runLifecycle(action) {
    if (!canControlService || serviceActionRunning) return false
    serviceActionRunning = true
    controlError = ""
    var id = core.action("lifecycle." + action, {}, function(ok, data, error) {
      root.serviceActionRunning = false
      if (!ok) root.controlError = root.actionError(error, root.tr("Could not update the Syncthing service"))
    })
    if (id) return true
    serviceActionRunning = false
    controlError = root.tr("The native core is not ready")
    return false
  }

  function toggleService() {
    return runLifecycle(serviceActive ? "stop" : "start")
  }

  function installSyncthing() { packageController.install() }

  // ---- conflict scan ------------------------------------------------------------
  // Walks every folder, so it runs when the folder set changes (not on every
  // snapshot: those arrive per sync event), plus every five minutes.
  function scanConflicts(force) {
    if (!online) return
    if (force !== true && folderKey === scannedFolderKey) return
    if (conflictScan.running || conflictInput !== "") { conflictRescan = true; return }
    var scanned = folders.filter(function(folder) { return !!folder.path })
    scannedFolderKey = folderKey
    if (!scanned.length) { conflictFolderIds = ({}); conflictScanError = ""; return }
    var payload = JSON.stringify(scanned.map(function(folder) { return folder.path }))
    // Bound UTF-8 below the helper's 1 MiB limit without putting paths in argv.
    if (payload.length > 262144) {
      conflictScanError = root.tr("Could not check for Syncthing conflicts")
      return
    }
    conflictScanFolders = scanned.map(function(folder) { return String(folder.id || "") })
    conflictInput = payload
    conflictOutput = ""
    conflictScan.stdinEnabled = true
    conflictDeadline.restart()
    conflictScan.running = true
  }
  function finishConflictScan(code) {
    conflictDeadline.stop()
    conflictInput = ""
    var flags = null
    try { flags = JSON.parse(conflictOutput.trim()) } catch (error) { flags = null }
    if (code !== 0 || !Array.isArray(flags) || flags.length !== conflictScanFolders.length) {
      // Keep the last known result; retry on the next folder change or timer.
      conflictScanError = root.tr("Could not check for Syncthing conflicts")
    } else {
      var next = ({})
      for (var i = 0; i < flags.length; i++) if (flags[i] === true) next[conflictScanFolders[i]] = true
      conflictScanError = ""
      conflictFolderIds = next
    }
    if (conflictRescan) {
      conflictRescan = false
      Qt.callLater(scanConflicts)
    }
  }

  onOnlineChanged: if (online) Qt.callLater(function() { root.scanConflicts(true) })
  onFolderKeyChanged: Qt.callLater(scanConflicts)
  onWidgetSettingsChanged: {
    settingsReady = true
    configureCore()
  }
  Component.onCompleted: Plugin.ServiceRegistry.instance = root
  Component.onDestruction: {
    conflictDeadline.stop()
    conflictInput = ""
    conflictScan.running = false
    if (Plugin.ServiceRegistry.instance === root) Plugin.ServiceRegistry.instance = null
  }

  property CoreProcess core: CoreProcess {
    pluginRoot: root.pluginRoot
    startupArguments: [
      "--desktop-authorized",
      "--probe-interval-seconds", String(root.probeIntervalSeconds),
      "--desired-service-state", root.configuredServiceState,
      "--lifecycle-kind", "systemd-user",
      "--lifecycle-authorized",
      "--lifecycle-unit", "syncthing.service"
    ]
    onProtocolReadyChanged: if (protocolReady) root.configureCore()
    onRunningChanged: {
      if (!running && root.folderMutationBusy)
        root.failFolderAction(null, root.tr("Folder operation stopped because the native core became unavailable"))
    }
  }

  property PackageController packageController: PackageController {
    helperPath: root.pluginRoot + "/scripts/syncthing-install.sh"
    probeIntervalSeconds: root.probeIntervalSeconds
    onStatusApplied: root.recoverCoreIfNeeded()
  }

  property Timer noticeTimer: Timer {
    interval: 10000
    onTriggered: root.folderMutationNotice = ""
  }

  property Timer conflictTimer: Timer {
    interval: 300000
    running: root.online
    repeat: true
    onTriggered: root.scanConflicts(true)
  }
  property Process conflictScan: Process {
    command: ["/usr/bin/python3", root.pluginRoot + "/scripts/syncthing-conflicts.py"]
    onStarted: {
      write(root.conflictInput + "\n")
      stdinEnabled = false
    }
    stdout: StdioCollector { onStreamFinished: root.conflictOutput = text }
    onExited: function(code) { root.finishConflictScan(code) }
  }
  property Timer conflictDeadline: Timer {
    interval: 30000
    onTriggered: {
      // Bound blocked filesystem calls and helper startup failures as well.
      if (root.conflictScan.running) root.conflictScan.signal(9)
      else root.finishConflictScan(1)
    }
  }

  // One diagnostics target per shell (the service is not per monitor).
  property FoamyIpc diagnostics: FoamyIpc {
    target: "foamy.syncthing.status"
    function state(): string {
      return JSON.stringify({ phase: root.phase, online: root.online,
        folders: root.folderCount, peers: root.connectedDeviceCount,
        refreshing: root.refreshing, conflicts: root.hasConflicts,
        error: root.lastError || root.conflictScanError })
    }
  }
}
