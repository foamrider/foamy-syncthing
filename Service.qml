import QtQuick
import Quickshell.Io
import "." as Plugin
import "hosts/omarchy"
import "Translations.js" as Translations

OmarchyService {
  id: root
  function tr(key, values) { return Translations.text(key, settings.language, values) }
  property bool hasConflicts: false
  property string conflictScanError: ""
  property string conflictOutput: ""

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

  function scanConflicts() {
    if (conflictScan.running || !online) return
    var paths = folders.map(function(folder) { return folder.path }).filter(Boolean)
    if (!paths.length) { hasConflicts = false; return }
    conflictOutput = ""
    conflictScan.command = ["bash", pluginRoot + "/hosts/omarchy/scripts/syncthing-conflicts.sh"].concat(paths)
    conflictScan.running = true
  }

  // Keep the existing-directory workflow; directory creation needs a new UI.
  onFolderDirectoryRequired: function(args) {
    folderMutationError = root.tr("The directory does not exist. Choose an existing directory.")
  }

  onOnlineChanged: if (online) Qt.callLater(scanConflicts)
  onFoldersChanged: Qt.callLater(scanConflicts)
  Component.onCompleted: Plugin.ServiceRegistry.instance = root
  Component.onDestruction: {
    if (Plugin.ServiceRegistry.instance === root) Plugin.ServiceRegistry.instance = null
  }
  property Timer conflictTimer: Timer {
    interval: 300000
    running: root.online
    repeat: true
    onTriggered: root.scanConflicts()
  }
  property Process conflictScan: Process {
    stdout: StdioCollector { onStreamFinished: root.conflictOutput = text }
    onExited: function(code) {
      var value = root.conflictOutput.trim()
      if (code !== 0 || (value !== "true" && value !== "false")) {
        root.conflictScanError = root.tr("Could not check for Syncthing conflicts")
        return
      }
      root.conflictScanError = ""
      root.hasConflicts = value === "true"
    }
  }
  property IpcHandler diagnostics: IpcHandler {
    enabled: root.settings.widgetId !== ""
    target: root.settings.widgetId + ".status"
    function state(): string {
      return JSON.stringify({ phase: root.phase, online: root.online,
        folders: root.folderCount, peers: root.connectedDeviceCount,
        refreshing: root.refreshing, conflicts: root.hasConflicts,
        error: root.lastError || root.conflictScanError })
    }
  }
}
