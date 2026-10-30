pragma ComponentBehavior: Bound
import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "." as Plugin
import "kit"
import "kit/FoamyPrefs.js" as Prefs
import "Preferences.js" as Schema
import "ui"

// Foamy Syncthing bar widget: one per bar (4.5 draws a bar per monitor). The
// native core, probes and scans live in Service.qml, once per shell, shared
// through ServiceRegistry; the FoamyMember leader hands it this widget's
// settings and owns the IPC target. The popup body is built on first open.
Panel {
  id: root

  ipcTarget: "foamy.syncthing"
  manageIpc: false

  property bool editingSettings: false
  property bool saving: false
  property string settingsError: ""
  property var lastSave: null
  property bool returningFromBrowser: false
  readonly property alias popup: popup
  readonly property var view: popup.bodyItem
  readonly property var syncthing: Plugin.ServiceRegistry.instance
  readonly property int openRefreshIntervalSec: Prefs.value(Schema.fields, root.settings, "openRefreshIntervalSec")

  FoamyMember { id: member; scope: "syncthing"; demanding: root.opened }
  Binding {
    target: root.syncthing
    property: "widgetSettings"
    value: root.settings
    when: member.isLeader && !!root.syncthing
    restoreMode: Binding.RestoreNone
  }

  FoamyIpc {
    target: "foamy.syncthing"
    enabled: member.isLeader
    function open(): void { root.open() }
    function close(): void { root.close() }
    function show(): void { root.open() }
    function hide(): void { root.close() }
    function toggle(): void { root.toggle() }
    function settings(): void { root.openSettings() }
    function overview(): void { root.closeSettings() }
  }

  // ---- bar state ---------------------------------------------------------------
  readonly property string iconVariant: {
    if (!syncthing || !syncthing.online) return "notify"
    if (syncthing.hasProblems || syncthing.tooFewDevices || syncthing.phase === "error") return "notify"
    if (syncthing.serviceAvailable && !syncthing.serviceActive) return "pause"
    if (syncthing.syncInProgress) return "sync"
    return "default"
  }
  readonly property string barIcon: iconVariant === "sync" ? "󰘿" : iconVariant === "default" ? "󰅠" : "󰧠"
  readonly property color iconColor: {
    if (syncthing && (syncthing.hasProblems || syncthing.phase === "error")) return bar ? bar.urgent : Commons.Color.urgent
    if (syncthing && syncthing.tooFewDevices) return FoamyPalette.warning
    return bar ? bar.foreground : Commons.Color.foreground
  }
  readonly property string tooltip: !syncthing ? FoamyI18n.t("Syncthing unavailable")
    : FoamyI18n.t("Syncthing: %1", [FoamyI18n.t(iconVariant === "sync" ? "Syncing files" : syncthing.compactSummary.title)])
      + "\n" + FoamyI18n.t("Left: details · Right: refresh")

  // ---- popup lifecycle -----------------------------------------------------------
  onOpenedChanged: {
    if (returningFromBrowser) return
    // Reset on close, so a later open (or the settings IPC) starts cleanly.
    if (!opened) editingSettings = false
    else if (syncthing) {
      syncthing.recover()
      syncthing.refresh()
    }
  }

  function openSettings() {
    settingsError = ""
    lastSave = null
    editingSettings = true
    if (!opened) open()
  }
  function closeSettings() { editingSettings = false }

  function save(key, value) {
    settingsError = ""
    var result = Prefs.save(root.bar, root.moduleName, Schema.fields, root.settings, key, value)
    lastSave = { key: key, value: value }
    if (!result.ok) { lastSave = null; settingsError = FoamyI18n.t(result.error); return }
    if (result.command.length === 0) return
    saving = true
    saveProcess.command = result.command
    saveProcess.running = true
  }
  function retrySave() { if (lastSave) save(lastSave.key, lastSave.value) }

  Process {
    id: saveProcess
    stderr: StdioCollector { id: saveError; waitForEnd: true }
    onExited: function(code) {
      root.saving = false
      root.settingsError = code === 0 ? "" : (String(saveError.text || "").trim().slice(0, 240) || FoamyI18n.t("Couldn't save the setting."))
    }
  }

  function openWebUi() {
    if (syncthing && syncthing.online && syncthing.baseUrl) Qt.openUrlExternally(syncthing.baseUrl)
  }
  function resolveFolderPath(value) {
    var home = Quickshell.env("HOME") || ""
    var path = String(value || "")
    if (path === "~") return home
    if (path.indexOf("~/") === 0) return home + path.slice(1)
    if (path.charAt(0) === "/" || !home) return path
    return home + "/" + path
  }
  function openFolder(folder) {
    var path = resolveFolderPath(folder ? folder.path : "")
    if (path) Quickshell.execDetached(["uwsm-app", "--", "xdg-open", path])
  }

  // The folder chooser is a separate window: release the popup while it is open.
  function browseFolder() {
    if (folderPicker.running) return
    returningFromBrowser = true
    root.close()
    folderPicker.running = true
  }

  Process {
    id: folderPicker
    command: ["bash", decodeURIComponent(Qt.resolvedUrl("scripts/syncthing-folder-picker.sh").toString().replace(/^file:\/\//, "")),
      FoamyI18n.t("Choose a Syncthing folder"), FoamyI18n.t("Choose")]
    stdout: StdioCollector { id: folderPickerOutput; waitForEnd: true }
    onExited: function(exitCode) {
      var match = String(folderPickerOutput.text).match(/(file:\/\/[^\r\n]*)/)
      root.open()
      if (exitCode !== 0) root.settingsError = FoamyI18n.t("The folder chooser failed; enter the path manually.")
      else if (match && root.view) root.view.setAddPath(decodeURIComponent(match[1].replace(/^file:\/\//, "")))
      Qt.callLater(function() { root.returningFromBrowser = false })
    }
  }

  // Native events update folders live; refresh service health while the popup is open.
  Timer {
    interval: root.openRefreshIntervalSec * 1000
    running: root.opened
    repeat: true
    onTriggered: {
      if (root.syncthing && !root.syncthing.refreshing && !root.syncthing.folderMutationBusy
          && !root.syncthing.serviceActionRunning) root.syncthing.refresh()
    }
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    iconComponent: Component {
      OpticalGlyph {
        anchors.fill: parent
        opacity: root.syncthing && root.syncthing.online ? 1.0 : 0.55
        text: root.barIcon
        color: root.iconColor
        fontFamily: button.fontFamily
        fontSize: button.fontSize
      }
    }
    active: !!root.syncthing && root.syncthing.hasProblems
    tooltipText: root.tooltip
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.RightButton && root.syncthing) {
        root.syncthing.recover()
        root.syncthing.refresh(true)
      } else root.toggle()
    }
  }

  FoamyPopup {
    id: popup
    anchorItem: button
    bar: root.bar
    owner: root
    open: root.opened
    body: Component {
      SyncthingView {
        service: root.syncthing
        settings: root.settings
        opened: root.opened
        editingSettings: root.editingSettings
        saving: root.saving
        settingsError: root.settingsError
        retryable: root.settingsError !== "" && root.lastSave !== null
        browsing: folderPicker.running || root.returningFromBrowser
        onCloseRequestedByView: root.close()
        onSettingsRequested: root.openSettings()
        onSettingsClosed: root.closeSettings()
        onSaveRequested: function(key, value) { root.save(key, value) }
        onRetryRequested: root.retrySave()
        onSwitchPanelRequested: function(direction) { root.switchPanel(direction) }
        onOpenWebUiRequested: root.openWebUi()
        onOpenFolderRequested: function(folder) { root.openFolder(folder) }
        onBrowseRequested: root.browseFolder()
      }
    }
  }
}
