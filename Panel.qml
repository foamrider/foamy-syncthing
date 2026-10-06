import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "." as Plugin
import "ui"
import "models/PanelModel.js" as PanelModel
import "Preferences.js" as Preferences
import "Translations.js" as Translations

Panel {
  id: root

  ipcTarget: moduleName
  manageIpc: false

  // Expose the direct settings view through the same panel lifecycle as the cog.
  IpcHandler {
    enabled: root.ipcTarget !== ""
    target: root.ipcTarget
    function open(): void { root.open() }
    function close(): void { root.close() }
    function show(): void { root.open() }
    function hide(): void { root.close() }
    function toggle(): void { root.toggle() }
    function settings(): void { root.open(); root.showSettings() }
    function overview(): void { root.hideSettings() }
  }

  property string widgetId: ""
  property string preferencesError: ""
  readonly property bool preferencesSaving: preferencesSave.running
  readonly property var preferences: Preferences.fromSettings(settings, Qt.locale().name)
  readonly property string language: preferences.language
  function tr(key, values) { return Translations.text(key, language, values) }
  function savePreference(key, value) {
    if (preferencesSave.running) return
    if (!widgetId) { preferencesError = "Could not read plugin settings"; return }
    preferencesError = ""
    preferencesSave.command = ["bash", localPathFromUrl(Qt.resolvedUrl("scripts/syncthing-preferences.sh")),
      widgetId, key, JSON.stringify(value)]
    preferencesSave.running = true
  }
  FileView {
    path: root.localPathFromUrl(Qt.resolvedUrl("manifest.json"))
    onLoaded: {
      try { root.widgetId = String(JSON.parse(text()).id || "") }
      catch (error) { root.preferencesError = "Could not read plugin settings" }
    }
    onLoadFailed: root.preferencesError = "Could not read plugin settings"
  }
  Process {
    id: preferencesSave
    onExited: function(code) { if (code !== 0) root.preferencesError = "Could not save plugin settings" }
  }

  readonly property var syncthing: Plugin.ServiceRegistry.instance
  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property bool lightTheme: Color.popups.background.hslLightness > 0.5
  readonly property color warning: lightTheme ? "#886000" : "#ebcb8b"
  readonly property color success: lightTheme ? "#3b6b30" : "#a3be8c"
  readonly property color syncthingBlue: lightTheme ? "#006a73" : "#26B6DB"
  readonly property color dim: Qt.darker(foreground, 1.5)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property string panelFontFamily: "sans-serif"
  readonly property color panelMuted: Qt.tint(Color.popups.background, Qt.rgba(foreground.r, foreground.g, foreground.b, 0.76))
  readonly property color panelOutline: Qt.tint(Color.popups.background, Qt.rgba(foreground.r, foreground.g, foreground.b, 0.22))
  readonly property color panelFill: Qt.tint(Color.popups.background, Qt.rgba(foreground.r, foreground.g, foreground.b, 0.055))
  readonly property string homePath: Quickshell.env("HOME")
  readonly property string folderPickerScript: localPathFromUrl(
    Qt.resolvedUrl("scripts/syncthing-folder-picker.sh"))
  readonly property bool folderPickerRunning: folderPickerProcess.running
  property bool moreOpen: false
  property bool addOpen: false
  property bool addIdEdited: false
  property bool addLabelFromOffer: false
  property bool addSubmissionPending: false
  property bool preserveStateForFolderPicker: false
  property string selectedFolderId: ""
  property string selectedPendingOffer: ""
  property string forgetFolderId: ""
  property bool forgetConfirmOpen: false
  property string folderPickerOutput: ""
  property string folderPickerError: ""
  property string displayedNotice: ""
  property bool noticeShown: false
  // The native core counts remote peers; our existing UI includes this computer.
  readonly property var deviceCounts: PanelModel.deviceCounts(syncthing)
  readonly property var folderRows: buildFolderRows()
  readonly property var compactSummary: PanelModel.compactSummary(syncthing, folderRows, tr)
  readonly property bool compactFolders: folderRows.length >= 5
  readonly property var visibleFolderRows: compactFolders
    ? (selectedFolder() ? [selectedFolder()] : []) : folderRows
  readonly property var pendingOfferRows: pendingOfferOptions()
  readonly property double trackedBytes: folderTotal("globalBytes")
  readonly property int trackedFiles: folderTotal("globalFiles")
  readonly property int scanningFolderCount: folderStateCount("scanning")
  readonly property int pausedFolderCount: folderStateCount("paused")
  readonly property bool syncInProgress: syncthing
    ? syncthing.syncingFolderCount > 0 || scanningFolderCount > 0
      || syncthing.syncingFiles.length > 0
    : false
  // A status refresh enters "loading" while the last known state remains usable.
  readonly property bool barStatusAvailable: syncthing
    ? syncthing.online
    : false
  readonly property bool hasProblems: syncthing
    ? syncthing.folderProblemCount > 0 || syncthing.hasConflicts : false
  readonly property bool hasConflict: syncthing
    ? syncthing.hasConflicts : false
  readonly property bool hasTooFewDevices: syncthing
    ? preferences.minimumConnectedDevices > 0 && deviceCounts.connected < preferences.minimumConnectedDevices : false
  readonly property string iconVariant: {
    if (!syncthing || !syncthing.online) return "notify"
    if (hasConflict || hasTooFewDevices || syncthing.phase === "error"
        || hasProblems) return "notify"
    if (syncthing.serviceAvailable && !syncthing.serviceActive) return "pause"
    if (syncInProgress) return "sync"
    return "default"
  }
  readonly property string barIcon: {
    if (iconVariant === "sync") return "󰘿"
    if (iconVariant === "default") return "󰅠"
    return "󰧠"
  }
  readonly property url syncthingIconSource: Qt.resolvedUrl("assets/syncthing.svg")
  readonly property string tooltip: {
    if (!syncthing) return root.tr("Syncthing unavailable")
    if (iconVariant === "sync" && syncInProgress) {
      return "Syncthing: " + root.tr("Syncing files")
    }
    return "Syncthing: " + root.tr(compactSummary.title)
  }
  readonly property string toggleHint: syncthing && syncthing.serviceActive
    ? "Stop syncing" : "Start syncing"
  readonly property string visibleError: {
    if (folderPickerError) return folderPickerError
    if (!syncthing) return ""
    return syncthing.folderMutationError || syncthing.controlError
      || syncthing.folderPreparationError || syncthing.packageError
      || syncthing.conflictScanError
      || syncthing.lastError || ""
  }
  readonly property color syncthingIconColor: {
    if (hasConflict) return urgent
    // The bar and popup can use different surfaces in the same theme.
    if (hasTooFewDevices) return Color.bar.background.hslLightness > 0.5 ? "#886000" : "#ebcb8b"
    return foreground
  }
  readonly property string visibleNotice: syncthing
    ? syncthing.folderMutationNotice : ""
  readonly property string visibleWarning: syncthing
    ? syncthing.recoveryWarning : ""
  readonly property string visibleSyncActivity: syncthing
    ? syncthing.syncActivity : ""
  readonly property string visibleSyncDots: syncthing
    ? syncthing.syncActivityDots : ""
  readonly property string visibleSyncAction: syncthing
    ? syncthing.syncActivityAction : ""
  readonly property string visibleSyncDetail: syncthing
    ? syncthing.syncActivityDetail : ""

  function configureService() {
    if (!syncthing) return
    syncthing.setRefreshInterval(Preferences.fromSettings(settings, Qt.locale().name).refreshIntervalSec)
  }

  function compactFolderState(folder) {
    return PanelModel.compactFolderState(folder, syncthing && syncthing.online,
      syncthing && syncthing.serviceAvailable && !syncthing.serviceActive, folderHasActivity(folder))
  }

  function showSettings() {
    moreOpen = true
    popup.scrollToTop()
    popup.focusPanel()
  }

  function hideSettings() {
    if (addOpen || forgetConfirmOpen) return
    popup.closeTransientPopups()
    moreOpen = false
    popup.scrollToTop()
    popup.focusPanel()
  }

  function buildFolderRows() {
    return PanelModel.buildFolderRows(syncthing, homePath)
  }

  function folderTotal(key) {
    return PanelModel.total(folderRows, key)
  }

  function folderStateCount(key) {
    return PanelModel.stateCount(folderRows, key)
  }

  function formatCount(value) {
    return PanelModel.formatCount(value)
  }

  function formatBytes(value) {
    return PanelModel.formatBytes(value)
  }

  function folderMeta(folder) {
    return PanelModel.folderMeta(folder, tr)
  }

  function folderState(folder) {
    return PanelModel.folderState(folder,
      syncthing ? syncthing.recentlyLinkedFolderId : "",
      folderHasActivity(folder))
  }

  function folderStateColor(folder) {
    var state = folderState(folder)
    if (state === "PAUSED") return warning
    if (state === "SYNCING") return warning
    if (state === "ERROR") return urgent
    return success
  }

  function folderHasActivity(folder) {
    return folder && syncthing && visibleSyncActivity !== ""
      && syncthing.syncActivityFolderId === folder.id
  }

  function selectedFolder() {
    return folderById(selectedFolderId)
  }

  function folderById(folderId) {
    return PanelModel.folderById(folderRows, folderId)
  }

  function selectActiveFolder() {
    if (!compactFolders || visibleSyncActivity === "" || !syncthing) return
    var activeFolder = folderById(syncthing.syncActivityFolderId)
    if (activeFolder) selectedFolderId = activeFolder.id
  }

  function ensureFolderSelection() {
    if (selectedFolder()) return
    selectedFolderId = folderRows.length > 0 ? folderRows[0].id : ""
  }

  function selectFolderOffset(offset) {
    if (folderRows.length < 2 || offset === 0) return
    var current = selectedFolder()
    var index = current ? folderRows.indexOf(current) : 0
    index = (index + (offset > 0 ? 1 : -1) + folderRows.length)
      % folderRows.length
    selectedFolderId = folderRows[index].id
  }

  function folderOptions() {
    return PanelModel.folderOptions(folderRows, homePath)
  }

  function deviceName(deviceId) {
    var devices = syncthing && syncthing.devices ? syncthing.devices : []
    return PanelModel.deviceName(devices, deviceId)
  }

  function deviceOptions() {
    return PanelModel.deviceOptions(syncthing, tr)
  }

  function pendingOfferOptions() {
    return PanelModel.pendingOfferOptions(syncthing)
  }

  function pendingFolderOptions() {
    var options = [{ value: "", label: root.tr("Create a new folder identity") }]
    for (var i = 0; i < pendingOfferRows.length; i++) {
      options.push({
        value: pendingOfferRows[i].value,
        label: root.tr("Accept {name}", {name: pendingOfferRows[i].label})
      })
    }
    return options
  }

  function ensurePendingOfferSelection() {
    for (var i = 0; i < pendingOfferRows.length; i++) {
      if (pendingOfferRows[i].value === selectedPendingOffer) return
    }
    selectedPendingOffer = pendingOfferRows.length > 0
      ? pendingOfferRows[0].value : ""
  }

  function encryptedPendingOfferCount() {
    return PanelModel.encryptedPendingOfferCount(syncthing)
  }

  function pathLabel(path) {
    return PanelModel.pathLabel(path)
  }

  function pathParentName(path) {
    return PanelModel.pathParentName(path, homePath)
  }

  function localPathFromUrl(url) {
    var value = String(url || "")
    if (value.indexOf("file://") === 0) value = value.slice(7)
    return decodeURIComponent(value)
  }

  function openAddFolder() {
    if (!syncthing || !syncthing.online || syncthing.folderMutationBusy) return
    syncthing.clearFolderMutationMessage()
    addOpen = true
    popup.scrollToTop()
    addIdEdited = false
    addLabelFromOffer = false
    addSubmissionPending = false
    popup.resetAddForm()
    syncthing.requestFolderIdSuggestion()
    Qt.callLater(function() { popup.focusAddPath() })
  }

  function closeAddFolder() {
    if (syncthing && syncthing.folderMutationBusy
        && syncthing.folderMutationAction === "add") return
    addOpen = false
    addSubmissionPending = false
    popup.scrollToTop()
    popup.focusPanel()
  }

  function resetTransientState() {
    moreOpen = false
    addOpen = false
    addIdEdited = false
    addLabelFromOffer = false
    addSubmissionPending = false
    forgetFolderId = ""
    forgetConfirmOpen = false
    folderPickerError = ""
    popup.closeTransientPopups()
  }

  function applyPendingFolder(value) {
    var selected = String(value || "")
    popup.selectedDeviceIds = []
    if (!selected) {
      addIdEdited = false
      popup.addIdText = ""
      if (addLabelFromOffer) popup.addLabelText = ""
      addLabelFromOffer = false
      if (syncthing) syncthing.requestFolderIdSuggestion()
      return
    }
    var choice
    try {
      choice = JSON.parse(selected)
    } catch (error) {
      return
    }
    if (!(choice instanceof Array) || choice.length !== 2) return
    var id = String(choice[0] || "")
    var deviceId = String(choice[1] || "")
    var pending = syncthing && syncthing.pendingFolders
      ? syncthing.pendingFolders[id] || ({}) : ({})
    var offeredBy = pending.offeredBy || ({})
    var offer = offeredBy[deviceId] || ({})
    addIdEdited = true
    popup.addIdText = id
    popup.addLabelText = String(offer.label || id)
    addLabelFromOffer = true
    popup.selectedDeviceIds = deviceId ? [deviceId] : []
  }

  function acceptPendingFolderOffer(value) {
    var selected = String(value || "")
    if (!selected) return
    openAddFolder()
    if (!addOpen) return
    popup.pendingFolderValue = selected
    applyPendingFolder(selected)
    popup.scrollToTop()
    Qt.callLater(function() { popup.focusAddPath() })
  }

  function selectedPendingDeviceId() {
    var value = String(popup.pendingFolderValue || "")
    if (!value) return ""
    try {
      var choice = JSON.parse(value)
      if (!(choice instanceof Array) || choice.length !== 2) return ""
      var deviceId = String(choice[1] || "")
      return popup.selectedDeviceIds.indexOf(deviceId) >= 0 ? deviceId : ""
    } catch (error) {
      return ""
    }
  }

  function submitAddFolder() {
    if (!syncthing || syncthing.folderMutationBusy) return
    var label = String(popup.addLabelText || "").trim()
    if (!label) label = pathLabel(popup.addPathText)
    selectedFolderId = String(popup.addIdText || "").trim()
    addSubmissionPending = syncthing.addFolder(
      popup.addPathText,
      label,
      popup.addIdText,
      popup.selectedDeviceIds,
      selectedPendingDeviceId())
  }

  function requestForget(folder) {
    if (!folder || !syncthing || !syncthing.online
        || syncthing.folderMutationBusy) return
    selectedFolderId = folder.id
    forgetFolderId = folder.id
    forgetConfirmOpen = true
    popup.focusPanel()
  }

  function confirmForget() {
    forgetConfirmOpen = false
    if (syncthing) syncthing.unlinkFolder(forgetFolderId)
    forgetFolderId = ""
  }

  function openWebUi() {
    if (syncthing && syncthing.online) Qt.openUrlExternally(syncthing.baseUrl)
  }

  function openFolder(folder) {
    var path = resolveFolderPath(folder ? folder.path : "")
    if (!path) return
    Quickshell.execDetached(["uwsm-app", "--", "xdg-open", path])
  }

  function browseForFolder() {
    if (folderPickerProcess.running) return
    folderPickerOutput = ""
    folderPickerError = ""
    folderPickerProcess.command = ["bash", folderPickerScript, language]
    preserveStateForFolderPicker = true
    close()
    folderPickerProcess.running = true
  }

  function resolveFolderPath(value) {
    var path = String(value || "")
    if (path === "~") return homePath
    if (path.indexOf("~/") === 0) return homePath + path.slice(1)
    if (path.charAt(0) === "/" || !homePath) return path
    return homePath + "/" + path
  }

  function toggleSyncing() {
    if (syncthing && syncthing.canControlService
        && !syncthing.serviceActionRunning) syncthing.toggleService()
  }

  function installationAction() {
    if (syncthing && syncthing.installationState === "missing") {
      syncthing.installSyncthing()
    }
  }

  onSyncthingChanged: configureService()
  onSettingsChanged: configureService()
  onFolderRowsChanged: {
    ensureFolderSelection()
    selectActiveFolder()
  }
  onPendingOfferRowsChanged: ensurePendingOfferSelection()
  onVisibleNoticeChanged: {
    if (visibleNotice !== "") {
      displayedNotice = visibleNotice
      noticeShown = true
      noticeFadeTimer.stop()
      noticeDisplayTimer.restart()
    } else if (displayedNotice !== "") {
      noticeDisplayTimer.stop()
      noticeShown = false
      noticeFadeTimer.restart()
    }
  }
  onOpenedChanged: {
    if (opened) {
      if (syncthing) syncthing.refresh()
      ensureFolderSelection()
      selectActiveFolder()
      popup.scrollToTop()
      Qt.callLater(function() { popup.focusPanel() })
    } else if (!preserveStateForFolderPicker) {
      resetTransientState()
    }
  }
  Component.onCompleted: configureService()

  // Native events update folders live; refresh service health while the panel is open.
  Timer {
    interval: root.preferences.openRefreshIntervalSec * 1000
    running: root.opened
    repeat: true
    onTriggered: {
      if (root.syncthing && !root.syncthing.refreshing
          && !root.syncthing.folderMutationBusy && !root.syncthing.serviceActionRunning)
        root.syncthing.refresh()
    }
  }

  Timer {
    id: noticeDisplayTimer
    interval: 10000
    repeat: false
    onTriggered: {
      root.noticeShown = false
      noticeFadeTimer.restart()
    }
  }

  Timer {
    id: noticeFadeTimer
    interval: 350
    repeat: false
    onTriggered: {
      if (root.noticeShown) return
      var expired = root.displayedNotice
      root.displayedNotice = ""
      if (root.syncthing
          && root.syncthing.folderMutationNotice === expired) {
        root.syncthing.clearFolderMutationNotice()
      }
    }
  }

  Connections {
    target: root.syncthing

    function onFolderIdSuggestionChanged() {
      if (root.addOpen && !root.addIdEdited
          && popup.pendingFolderValue === "") {
        popup.addIdText = root.syncthing.folderIdSuggestion
      }
    }

    function onFolderMutationNoticeChanged() {
      if (root.addSubmissionPending
          && root.syncthing.folderMutationNotice !== "") {
        root.addOpen = false
        root.addSubmissionPending = false
        popup.focusPanel()
      }
    }

    function onFolderMutationErrorChanged() {
      if (root.syncthing.folderMutationError !== "") {
        root.addSubmissionPending = false
      }
    }

    function onSyncActivityFolderIdChanged() {
      root.selectActiveFolder()
    }
  }

  Process {
    id: folderPickerProcess
    command: []

    stdout: StdioCollector {
      id: folderPickerStdout
      waitForEnd: true
      onStreamFinished: root.folderPickerOutput = text
    }

    onExited: function(exitCode) {
      var selected = String(root.folderPickerOutput
        || folderPickerStdout.text || "").trim()
      if (exitCode === 0 && selected) {
        var path = root.localPathFromUrl(selected)
        popup.addPathText = path
        if (!popup.addLabelText) popup.addLabelText = root.pathLabel(path)
      } else if (exitCode !== 0) {
        root.folderPickerError = "Folder chooser failed; enter the path manually."
      }
      Qt.callLater(function() {
        root.preserveStateForFolderPicker = false
        root.open()
        if (root.addOpen) {
          Qt.callLater(function() { popup.focusAddPath() })
        }
      })
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
        opacity: root.barStatusAvailable ? 1.0 : 0.55
        text: root.barIcon
        color: root.syncthingIconColor
        fontFamily: button.fontFamily
        fontSize: button.fontSize
      }
    }
    active: root.hasProblems
    tooltipText: root.tooltip
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.RightButton && root.syncthing) root.syncthing.refresh()
      else root.toggle()
    }
  }

  SyncthingPanelPopup {
    id: popup
    anchorItem: button
    controller: root
    owner: root
    bar: root.bar
    open: root.opened
  }

}
