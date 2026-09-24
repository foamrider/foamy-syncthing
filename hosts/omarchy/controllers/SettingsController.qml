import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import "../../../Preferences.js" as Preferences

// The native adapter's settings boundary. Omarchy owns persistence; this
// controller watches the widget entry and never writes Syncthing configuration.
QtObject {
  id: root
  property var selectTheme
  property bool runtimeReady: false
  property string currentWebUiTheme: ""
  property string guiAssetsPath: ""
  property string widgetId: ""
  property var widgetSettings: ({})
  property bool loaded: false
  property string error: ""
  property string notice: ""
  readonly property var preferences: Preferences.fromSettings(widgetSettings, Qt.locale().name)
  readonly property string language: preferences.language
  readonly property bool settingsReady: loaded && widgetId !== ""
  readonly property bool busy: save.running
  readonly property string serviceState: preferences.serviceState
  readonly property int probeIntervalSeconds: preferences.probeIntervalSec
  readonly property bool serviceStateActionRunning: save.running
  property string iconStyle: "themed"
  readonly property bool migrationOpen: false
  readonly property bool canAutoPort: false
  readonly property string migrationMessage: ""
  readonly property color warning: palette.warning
  readonly property color success: palette.success
  readonly property color syncActivity: palette.syncActivity
  readonly property string settingsPath: (Quickshell.env("XDG_CONFIG_HOME")
    || Quickshell.env("HOME") + "/.config") + "/omarchy/shell.json"

  function localPath(url) { return decodeURIComponent(String(url).replace(/^file:\/\//, "")) }
  function recheckSettings() { root.config.reload() }
  function clearNotice() { notice = "" }
  function setLegacyThemedIcon(enabled) { iconStyle = enabled ? "themed" : "branded" }
  function openSettings() { Quickshell.execDetached(["omarchy-shell", widgetId, "settings"]) }
  function setServiceState(value) {
    if (!settingsReady || save.running || ["enabled", "disabled"].indexOf(value) < 0) return false
    save.command = ["bash", localPath(Qt.resolvedUrl("../../../scripts/syncthing-preferences.sh")),
      widgetId, "serviceState", JSON.stringify(value)]
    save.running = true
    return true
  }
  // The inherited adapter exposes these operations, but this plugin has no
  // TOML migration or private uninstall path. Keep them explicit if invoked.
  function autoPort() { error = "Plugin preferences are stored in Omarchy shell.json" }
  function manualPort() { openSettings() }
  function cancelMigration() { error = "" }
  function requestSelfRemoval(purge) { error = "Remove this plugin with omarchy plugin remove " + widgetId }

  property ThemePaletteController palette: ThemePaletteController {}
  property FileView manifest: FileView {
    path: root.localPath(Qt.resolvedUrl("../../../manifest.json"))
    onLoaded: {
      try { root.widgetId = String(JSON.parse(text()).id || ""); root.config.reload() }
      catch (error) { root.error = "Could not read plugin settings" }
    }
    onLoadFailed: root.error = "Could not read plugin settings"
  }
  property FileView config: FileView {
    path: root.settingsPath
    watchChanges: true
    printErrors: false
    onLoaded: {
      try {
        root.widgetSettings = Preferences.widgetSettings(JSON.parse(text()), root.widgetId)
        root.loaded = true
        root.error = ""
      } catch (error) { root.error = "Could not read plugin settings" }
    }
    onLoadFailed: function(error) {
      if (error === FileViewError.FileNotFound) { root.widgetSettings = ({}); root.loaded = true }
      else root.error = "Could not read plugin settings"
    }
    onFileChanged: reload()
  }
  property Process save: Process {
    onExited: function(code) {
      if (code !== 0) root.error = "Could not save plugin settings"
      else root.config.reload()
    }
  }
  Component.onCompleted: palette.refreshNow()
}
