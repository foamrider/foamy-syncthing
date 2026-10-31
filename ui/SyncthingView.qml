pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "../kit"
import "../kit/FoamyPrefs.js" as Prefs
import "../Preferences.js" as Schema
import "../models/PanelModel.js" as PanelModel
import "../models/OverviewModel.js" as OverviewModel
import "../models/Keyed.js" as Keyed

// The popup body: header, summary, Folders/Devices tabs with expandable rows
// (one keyboard/pointer cursor), the settings page, the add-folder page and
// the unlink confirmation. Data comes from `service` (Service.qml through
// ServiceRegistry, or a mock in preview/Preview.qml). Rows are keyed
// ListModels updated in place, built only while the popup is open.
PanelKeyCatcher {
  id: root

  property QtObject service: null
  property var settings: ({})
  property bool opened: true
  property bool editingSettings: false
  property bool saving: false
  property string settingsError: ""
  property bool retryable: false
  // The folder chooser is open (the popup closes meanwhile; keep the form).
  property bool browsing: false

  signal closeRequestedByView()
  signal settingsRequested()
  signal settingsClosed()
  signal saveRequested(string key, var value)
  signal retryRequested()
  signal switchPanelRequested(int direction)
  signal openWebUiRequested()
  signal openFolderRequested(var folder)
  signal browseRequested()

  property string tab: "folders"
  property var expanded: ({})
  property bool addOpen: false
  property string forgetFolderId: ""
  property string selectedPendingOffer: ""
  property string lastNotice: ""

  readonly property bool online: !!service && service.online
  readonly property bool canManage: online && !service.folderMutationBusy
  readonly property var summary: service ? service.compactSummary : ({ title: "Syncthing unavailable", detail: "", progress: -1 })
  readonly property var counts: service ? service.deviceCounts : ({ connected: 0, total: 0 })
  readonly property var folderRows: service ? service.folderRows : []
  readonly property var pendingOffers: service ? service.pendingOfferRows : []
  readonly property string notice: service ? service.folderMutationNotice : ""
  readonly property string errorText: {
    if (!service) return ""
    return service.folderMutationError || service.controlError || service.folderPreparationError
      || service.packageError || service.conflictScanError || service.lastError || ""
  }
  readonly property string deviceLine: !service ? FoamyI18n.t("Syncthing unavailable")
    : service.online && service.serviceActive ? FoamyI18n.t("{connected} / {total} devices online", counts)
    : service.installationState === "missing" ? FoamyI18n.t("Syncthing is not installed")
    : !service.serviceActive ? FoamyI18n.t("Service is not running")
    : FoamyI18n.t("Syncthing unavailable")

  // ---- keyed rows (built only while shown) -------------------------------------
  readonly property var liveRows: opened && !editingSettings && !addOpen
    ? OverviewModel.rows(service, folderRows, FoamyI18n.t, PanelModel.formatBytes, PanelModel.formatCount, FoamyI18n.tn)
    : null
  readonly property var liveSettingsRows: opened && editingSettings ? folderRows : null
  property var folderMap: ({})
  property var deviceMap: ({})
  property var settingsMap: ({})
  ListModel { id: folderKeys }
  ListModel { id: deviceKeys }
  ListModel { id: settingsKeys }
  onLiveRowsChanged: {
    if (!liveRows) return
    folderMap = Keyed.index(liveRows.folders)
    deviceMap = Keyed.index(liveRows.devices)
    Keyed.sync(folderKeys, Keyed.keys(liveRows.folders))
    Keyed.sync(deviceKeys, Keyed.keys(liveRows.devices))
  }
  onLiveSettingsRowsChanged: {
    if (!liveSettingsRows) return
    settingsMap = Keyed.index(liveSettingsRows)
    Keyed.sync(settingsKeys, Keyed.keys(liveSettingsRows))
  }
  onNoticeChanged: if (notice !== "") lastNotice = notice

  // ---- cursor: tabs, rows, then the header actions ------------------------------
  readonly property int rowCount: tab === "folders" ? folderKeys.count : deviceKeys.count
  readonly property int firstRowStop: 2
  readonly property int toggleStop: firstRowStop + rowCount
  readonly property int webStop: toggleStop + 1
  readonly property int settingsStop: toggleStop + 2
  FoamyCursor { id: cursor; count: root.settingsStop + 1 }

  implicitWidth: FoamyStyle.popupWidth
  implicitHeight: !editingSettings ? content.implicitHeight
    : addOpen ? addPage.implicitHeight : settingsPage.implicitHeight
  blocked: editingSettings || addOpen || forgetFolderId !== ""

  onMoveRequested: function(dx, dy) {
    if (dy !== 0) cursor.move(dy)
    else if (cursor.index >= 0 && cursor.index < firstRowStop) {
      setTab(dx > 0 ? "devices" : "folders")
      cursor.hover(dx > 0 ? 1 : 0)
    } else cursor.move(dx)
  }
  onActivateRequested: root.activate(cursor.index)
  onCloseRequested: root.closeRequestedByView()
  onTabRequested: function(direction) { root.switchPanelRequested(direction) }
  onTextKey: function(text, modifiers) {
    // Ctrl/Alt/Meta shortcuts belong to the shell (modifiers is undefined on 4.0).
    if (!cursor.plain(modifiers)) return
    var key = text.toLowerCase()
    if (key === "s") root.settingsRequested()
    else if (key === "r" && root.service) { root.service.recover(); root.service.refresh(true) }
    else if (key === "w") root.openWebUiRequested()
    else if (key === "p") root.toggleService()
    else if (key === "o") {
      var row = root.cursorRow()
      if (row && row.folder) root.openFolderRequested(row.folder)
    }
  }
  onOpenedChanged: {
    if (opened) return
    cursor.reset()
    if (!browsing) {
      addOpen = false
      forgetFolderId = ""
    }
  }
  onEditingSettingsChanged: {
    if (editingSettings) {
      settingsPage.resetScroll()
      Qt.callLater(settingsPage.focusBack)
    } else {
      addOpen = false
      forgetFolderId = ""
      Qt.callLater(function() { root.forceActiveFocus() })
    }
  }

  Connections {
    target: cursor
    function onIndexChanged() {
      var i = cursor.index - root.firstRowStop
      if (i < 0 || i >= root.rowCount) return
      var item = root.rowItem(i)
      if (item) root.keepVisible(item)
    }
  }

  // The overview list's wheel scrolling (tests).
  readonly property QtObject listWheel: wheel
  // The delegate for overview row `index` (tests, scrolling).
  function rowItem(index) { return rowRepeater.itemAt(index) }

  function setTab(value) {
    if (tab === value) return
    tab = value
    wheel.scrollTo(0, false)
  }
  function rowKey(index) {
    var model = tab === "folders" ? folderKeys : deviceKeys
    return index >= 0 && index < model.count ? model.get(index).key : ""
  }
  function cursorRow() {
    var key = rowKey(cursor.index - firstRowStop)
    return key ? (tab === "folders" ? folderMap : deviceMap)[key] : null
  }
  function toggleRow(key) {
    var next = Object.assign({}, expanded)
    next[tab + ":" + key] = !next[tab + ":" + key]
    expanded = next
    if (!next[tab + ":" + key]) return
    // Scroll the opened row's details into view as they grow.
    var model = tab === "folders" ? folderKeys : deviceKeys
    for (var i = 0; i < model.count; i++) {
      if (model.get(i).key !== key) continue
      var item = rowItem(i)
      if (item) Qt.callLater(function() { wheel.ensureVisible(item) })
      return
    }
  }
  function keepVisible(item) {
    var top = item.y
    var bottom = item.y + item.height
    if (top < list.contentY) list.contentY = top
    else if (bottom > list.contentY + list.height) list.contentY = bottom - list.height
  }
  function activate(index) {
    if (index < 0) return
    if (index === 0) setTab("folders")
    else if (index === 1) setTab("devices")
    else if (index < toggleStop) toggleRow(rowKey(index - firstRowStop))
    else if (index === toggleStop) toggleService()
    else if (index === webStop) openWebUiRequested()
    else if (index === settingsStop) settingsRequested()
  }
  function toggleService() {
    if (!service) return
    if (service.canInstall) service.installSyncthing()
    else if (service.canControlService && !service.serviceActionRunning && !service.folderMutationBusy) service.toggleService()
  }

  // Add folder: opened from Settings; the Panel fills the path after browsing.
  function openAdd(pendingValue) {
    if (!canManage) return
    service.clearFolderMutationMessage()
    addForm.reset()
    addOpen = true
    service.requestFolderIdSuggestion()
    if (pendingValue) addForm.applyPendingFolder(pendingValue)
    addPage.resetScroll()
    Qt.callLater(addForm.focusPath)
  }
  function closeAdd() {
    if (service && service.folderMutationBusy && service.folderMutationAction === "add") return
    addOpen = false
    Qt.callLater(settingsPage.focusBack)
  }
  function setAddPath(path) {
    if (!addOpen) return
    addForm.setPath(path)
  }
  function requestUnlink(folder) {
    if (!folder || !canManage) return
    forgetFolderId = folder.id
    confirm.selectedIndex = 0
    Qt.callLater(confirm.forceActiveFocus)
  }
  function confirmUnlink() {
    var id = forgetFolderId
    forgetFolderId = ""
    if (service && id) service.unlinkFolder(id)
    Qt.callLater(settingsPage.focusBack)
  }
  function cancelUnlink() {
    forgetFolderId = ""
    Qt.callLater(settingsPage.focusBack)
  }

  function statusColor(state) {
    if (state === "Error" || state === "Out of sync" || state === "Conflicts") return FoamyPalette.onSurface(FoamyPalette.error)
    if (state === "Syncing" || state === "Scanning" || state === "Checking") return Commons.Color.accent
    if (state === "Connected" || state === "Up to date") return FoamyPalette.onSurface(FoamyPalette.success)
    if (state === "Paused" || state === "Waiting to sync") return FoamyPalette.onSurface(FoamyPalette.warning)
    return FoamyPalette.mutedText
  }
  function statusIcon(state) {
    if (state === "Error" || state === "Out of sync" || state === "Conflicts") return "alert"
    if (state === "Connected" || state === "Up to date") return "check"
    if (state === "Paused") return "pause"
    if (state === "Disconnected" || state === "Unavailable") return "disconnected"
    return "sync"
  }
  function folderStateColor(folder) {
    if (!folder) return FoamyPalette.mutedText
    if (folder.outOfSync || folder.problem || folder.conflict) return FoamyPalette.onSurface(FoamyPalette.error)
    if (folder.paused) return FoamyPalette.onSurface(FoamyPalette.warning)
    if (folder.syncing || folder.scanning) return Commons.Color.accent
    return FoamyPalette.onSurface(FoamyPalette.success)
  }

  component Label: Text {
    textFormat: Text.PlainText
    horizontalAlignment: Text.AlignLeft
    color: FoamyPalette.text
    font.family: FoamyStyle.fontFamily
    font.pixelSize: FoamyStyle.font.body
  }

  // ---- overview -------------------------------------------------------------------
  ColumnLayout {
    id: content
    anchors.fill: parent
    visible: !root.editingSettings
    spacing: 0

    // The foamy hero band: title row (devices online, start/stop, Web UI,
    // Settings) and the sync summary with its progress ring as the hero.
    FoamyHeroBand {
      id: band
      Layout.fillWidth: true
      iconName: "sync"
      title: FoamyI18n.t("Syncthing")
      meta: root.deviceLine
      settingsHasCursor: cursor.at(root.settingsStop)
      onSettingsClicked: root.settingsRequested()
      onSettingsHovered: function(hovered) { if (hovered) cursor.hover(root.settingsStop) }
      trailing: [
        FoamyAction {
          iconName: root.service && root.service.canInstall ? "download"
            : root.service && root.service.serviceActive ? "pause" : "play"
          tooltipText: root.service && root.service.canInstall ? FoamyI18n.t("Install Syncthing")
            : FoamyI18n.t(root.service && root.service.serviceActive ? "Stop syncing" : "Start syncing") + " · P"
          spinning: root.opened && !!root.service && root.service.serviceActionRunning
          actionEnabled: !!root.service && (root.service.canInstall || root.service.canControlService)
            && !root.service.serviceActionRunning && !root.service.folderMutationBusy
          hasCursor: cursor.at(root.toggleStop)
          foreground: FoamyPalette.secondaryText
          onHoveredChanged: if (hovered) cursor.hover(root.toggleStop)
          onClicked: root.toggleService()
        },
        FoamyAction {
          iconName: "globe"
          tooltipText: FoamyI18n.t("Open the Syncthing Web UI") + " · W"
          actionEnabled: root.online
          hasCursor: cursor.at(root.webStop)
          foreground: FoamyPalette.secondaryText
          onHoveredChanged: if (hovered) cursor.hover(root.webStop)
          onClicked: root.openWebUiRequested()
        }
      ]

      // Hero: state, detail, progress ring and the tracked totals.
      RowLayout {
        Layout.fillWidth: true
        spacing: FoamyStyle.gapLarge
        ColumnLayout {
          Layout.fillWidth: true
          Layout.minimumWidth: 0
          spacing: FoamyStyle.gapSmall
          Label {
            Layout.fillWidth: true
            text: FoamyI18n.t(root.summary.title)
            wrapMode: Text.WordWrap
            color: root.service && root.service.hasProblems ? Commons.Color.urgent : FoamyPalette.text
            font.pixelSize: FoamyStyle.font.display
          }
          Label {
            Layout.fillWidth: true
            visible: text !== ""
            text: FoamyI18n.t(root.summary.detail)
            wrapMode: Text.WordWrap
            color: FoamyPalette.secondaryText
            font.pixelSize: FoamyStyle.font.bodySmall
          }
          Label {
            Layout.fillWidth: true
            visible: root.online && root.folderRows.length > 0
            text: FoamyI18n.t("{count} files · {size}", {
              count: PanelModel.formatCount(PanelModel.total(root.folderRows, "globalFiles")),
              size: PanelModel.formatBytes(PanelModel.total(root.folderRows, "globalBytes")) })
            color: FoamyPalette.secondaryText
            font.pixelSize: FoamyStyle.font.body
          }
        }
        SyncthingProgress {
          Layout.preferredWidth: Style.space(64)
          Layout.preferredHeight: Style.space(64)
          Layout.alignment: Qt.AlignVCenter
          progress: root.summary.progress
        }
      }
    }

    // Warnings, errors, the last folder action and installer progress.
    ColumnLayout {
      Layout.fillWidth: true
      Layout.leftMargin: FoamyStyle.padding
      Layout.rightMargin: FoamyStyle.padding
      Layout.topMargin: visibleChildren.length > 0 ? FoamyStyle.gapLarge : 0
      spacing: FoamyStyle.gapSmall
      Label {
        Layout.fillWidth: true
        visible: text !== ""
        text: root.service ? root.service.recoveryWarning : ""
        wrapMode: Text.WordWrap
        color: FoamyPalette.onSurface(FoamyPalette.warning)
        font.pixelSize: FoamyStyle.font.bodySmall
      }
      Label {
        Layout.fillWidth: true
        visible: text !== ""
        text: root.service && root.service.tooFewDevices
          ? FoamyI18n.tn("Fewer than %1 devices are connected", Prefs.value(Schema.fields, root.settings, "minimumConnectedDevices")) : ""
        wrapMode: Text.WordWrap
        color: FoamyPalette.onSurface(FoamyPalette.warning)
        font.pixelSize: FoamyStyle.font.bodySmall
      }
      Label {
        Layout.fillWidth: true
        visible: text !== ""
        text: root.errorText
        wrapMode: Text.WordWrap
        color: Commons.Color.urgent
        font.pixelSize: FoamyStyle.font.bodySmall
        Accessible.role: Accessible.AlertMessage
      }
      Label {
        Layout.fillWidth: true
        visible: text !== ""
        text: root.service ? FoamyI18n.t(root.service.packageStatus) : ""
        wrapMode: Text.WordWrap
        color: FoamyPalette.secondaryText
        font.pixelSize: FoamyStyle.font.bodySmall
      }
      Label {
        Layout.fillWidth: true
        visible: opacity > 0
        opacity: root.notice !== "" ? 1 : 0
        text: root.lastNotice
        wrapMode: Text.WordWrap
        color: FoamyPalette.onSurface(FoamyPalette.success)
        font.pixelSize: FoamyStyle.font.bodySmall
        Behavior on opacity { NumberAnimation { duration: FoamyMotion.slow } }
      }
    }

    // Tabs
    RowLayout {
      Layout.fillWidth: true
      Layout.topMargin: FoamyStyle.gapLarge
      Layout.leftMargin: FoamyStyle.gapLarge
      Layout.rightMargin: FoamyStyle.gapLarge
      spacing: FoamyStyle.gapSmall
      TabStop { view: "folders"; iconName: "folder"; stop: 0; label: FoamyI18n.t("Folders"); count: folderKeys.count }
      TabStop { view: "devices"; iconName: "devices"; stop: 1; label: FoamyI18n.t("Devices"); count: deviceKeys.count }
    }

    Flickable {
      id: list
      Layout.fillWidth: true
      Layout.fillHeight: true
      Layout.minimumHeight: 0
      Layout.topMargin: FoamyStyle.gapSmall
      Layout.bottomMargin: FoamyStyle.gapLarge
      implicitHeight: rows.implicitHeight
      contentWidth: width
      contentHeight: rows.implicitHeight
      clip: true
      boundsBehavior: Flickable.StopAtBounds
      interactive: contentHeight > height
      ScrollBar.vertical: ScrollBar {
        visible: list.interactive
        width: FoamyStyle.gapSmall
        padding: 0
        contentItem: Rectangle { implicitWidth: FoamyStyle.gapSmall; radius: FoamyStyle.pillRadius(width); color: FoamyPalette.mutedText }
        background: Item {}
      }

      // A wheel notch scrolls about three rows, however the mouse splits it.
      FoamyWheelScroll { id: wheel; target: list }

      Column {
        id: rows
        width: list.width
        spacing: FoamyStyle.gapTiny

        Label {
          visible: root.rowCount === 0
          x: FoamyStyle.padding
          width: parent.width - FoamyStyle.padding * 2
          topPadding: FoamyStyle.gap
          bottomPadding: FoamyStyle.gap
          text: !root.online ? FoamyI18n.t("Current status is unavailable.")
            : root.tab === "folders" ? FoamyI18n.t("No shared folders yet. Add one in Settings.")
            : FoamyI18n.t("No remote devices configured.")
          wrapMode: Text.WordWrap
          color: FoamyPalette.mutedText
          font.pixelSize: FoamyStyle.font.bodySmall
        }

        Repeater {
          id: rowRepeater
          model: root.tab === "folders" ? folderKeys : deviceKeys
          delegate: OverviewRow {}
        }
      }
    }
  }

  // ---- settings ---------------------------------------------------------------------
  FoamySettingsPage {
    id: settingsPage
    visible: root.editingSettings && !root.addOpen
    width: parent.width
    height: parent.height
    saving: root.saving
    error: root.settingsError
    retryable: root.retryable
    onBack: root.settingsClosed()
    onRetry: root.retryRequested()

    FoamySettingsSection {
      width: parent.width
      title: FoamyI18n.t("Shared folders")
      description: FoamyI18n.t("Pause keeps a folder configured. Unlink removes it from Syncthing and keeps the local files.")

      Label {
        visible: settingsKeys.count === 0
        width: parent.width
        text: root.online ? FoamyI18n.t("No shared folders yet. Add an existing directory to get started.")
          : FoamyI18n.t("Connect to Syncthing to manage folders.")
        wrapMode: Text.WordWrap
        color: FoamyPalette.mutedText
        font.pixelSize: FoamyStyle.font.bodySmall
      }
      Repeater {
        model: settingsKeys
        delegate: SettingsFolderRow {}
      }
      Label {
        visible: text !== ""
        width: parent.width
        text: root.service ? root.service.folderMutationError : ""
        wrapMode: Text.WordWrap
        color: Commons.Color.urgent
        font.pixelSize: FoamyStyle.font.bodySmall
        Accessible.role: Accessible.AlertMessage
      }
      Label {
        visible: opacity > 0
        opacity: root.notice !== "" ? 1 : 0
        width: parent.width
        text: root.lastNotice
        wrapMode: Text.WordWrap
        color: FoamyPalette.onSurface(FoamyPalette.success)
        font.pixelSize: FoamyStyle.font.bodySmall
        Behavior on opacity { NumberAnimation { duration: FoamyMotion.slow } }
      }
      FoamyAction {
        iconName: "plus"
        label: FoamyI18n.t("Add folder")
        focusable: true
        actionEnabled: root.canManage
        tooltipText: root.online ? "" : FoamyI18n.t("Connect to Syncthing to manage folders.")
        foreground: FoamyPalette.secondaryText
        onClicked: root.openAdd("")
      }
    }

    FoamySettingsSection {
      visible: root.pendingOffers.length > 0
      width: parent.width
      title: FoamyI18n.t("Incoming shares")
      RowLayout {
        width: parent.width
        spacing: FoamyStyle.gap
        FoamyDropdown {
          Layout.fillWidth: true
          showLabel: false
          label: FoamyI18n.t("Incoming shares")
          value: root.selectedPendingOffer
          options: root.pendingOffers
          onChanged: function(value) { root.selectedPendingOffer = value }
        }
        FoamyAction {
          iconName: "check"
          focusable: true
          tooltipText: FoamyI18n.t("Accept incoming share")
          actionEnabled: root.canManage && root.selectedPendingOffer !== ""
          foreground: FoamyPalette.secondaryText
          onClicked: root.openAdd(root.selectedPendingOffer)
        }
      }
    }

    FoamySettingsSection {
      width: parent.width
      title: FoamyI18n.t("Warnings")
      FoamyDropdown {
        width: parent.width
        label: FoamyI18n.t("Warn when fewer devices are connected (this one included)")
        value: String(Prefs.value(Schema.fields, root.settings, "minimumConnectedDevices"))
        options: {
          var current = Prefs.value(Schema.fields, root.settings, "minimumConnectedDevices")
          var rows = [{ value: "0", label: FoamyI18n.t("No warning") }]
          var maximum = Math.min(100, Math.max(5, root.counts.total, current))
          for (var i = 1; i <= maximum; ++i) rows.push({ value: String(i), label: FoamyI18n.tn("%1 devices", i) })
          return rows
        }
        onChanged: function(value) { root.saveRequested("minimumConnectedDevices", Number(value)) }
      }
    }

    FoamySettingsSection {
      width: parent.width
      title: FoamyI18n.t("Refresh")
      description: FoamyI18n.t("File activity arrives live from Syncthing; these refresh the full status.")
      FoamyDropdown {
        width: parent.width
        label: FoamyI18n.t("Refresh every")
        value: String(Prefs.value(Schema.fields, root.settings, "refreshIntervalSec"))
        options: [
          { value: "60", label: FoamyI18n.tn("%1 minutes", 1) },
          { value: "300", label: FoamyI18n.tn("%1 minutes", 5) },
          { value: "900", label: FoamyI18n.tn("%1 minutes", 15) },
          { value: "3600", label: FoamyI18n.tn("%1 hours", 1) }
        ]
        onChanged: function(value) { root.saveRequested("refreshIntervalSec", Number(value)) }
      }
      FoamyDropdown {
        width: parent.width
        label: FoamyI18n.t("Refresh while the popup is open every")
        value: String(Prefs.value(Schema.fields, root.settings, "openRefreshIntervalSec"))
        options: [
          { value: "2", label: FoamyI18n.tn("%1 seconds", 2) },
          { value: "5", label: FoamyI18n.tn("%1 seconds", 5) },
          { value: "10", label: FoamyI18n.tn("%1 seconds", 10) },
          { value: "30", label: FoamyI18n.tn("%1 seconds", 30) }
        ]
        onChanged: function(value) { root.saveRequested("openRefreshIntervalSec", Number(value)) }
      }
    }
  }

  // ---- add folder ---------------------------------------------------------------------
  FoamySettingsPage {
    id: addPage
    visible: root.editingSettings && root.addOpen
    width: parent.width
    height: parent.height
    title: FoamyI18n.t("Add folder")
    saving: !!root.service && root.service.folderMutationBusy && root.service.folderMutationAction === "add"
    onBack: root.closeAdd()

    AddFolderForm {
      id: addForm
      width: parent.width
      service: root.service
      browsing: root.browsing
      onBrowseRequested: root.browseRequested()
      onAdded: root.closeAdd()
    }
  }

  // ---- unlink confirmation --------------------------------------------------------------
  UnlinkDialog {
    id: confirm
    anchors.fill: parent
    z: 10
    opened: root.forgetFolderId !== ""
    message: {
      var folder = PanelModel.folderById(root.folderRows, root.forgetFolderId)
      return folder
        ? FoamyI18n.t("Unlink {name} ({id}) from Syncthing?", {name: folder.label, id: folder.id}) + "\n\n"
          + FoamyI18n.t("This removes its Syncthing configuration. Local files will be kept.") + " "
          + (folder.markerName === ".stfolder" ? FoamyI18n.t("Syncthing may remove its internal .stfolder marker.") + " " : "")
          + FoamyI18n.t("Keep this folder ID to rejoin the same remote folder.")
        : FoamyI18n.t("Unlink this folder from Syncthing? Local files will be kept.")
    }
    confirmText: FoamyI18n.t("Unlink")
    cancelText: FoamyI18n.t("Cancel")
    onCanceled: root.cancelUnlink()
    onConfirmed: root.confirmUnlink()
  }

  // ---- components ----------------------------------------------------------------------
  component TabStop: FoamyCursorSurface {
    id: tabStop
    required property string view
    required property string iconName
    required property int stop
    required property string label
    required property int count
    Layout.fillWidth: true
    implicitHeight: FoamyStyle.rowHeight
    hasCursor: cursor.at(stop)
    current: root.tab === view
    onHovered: cursor.hover(stop)
    onClicked: root.setTab(view)
    Accessible.role: Accessible.PageTab
    Accessible.name: label
    Accessible.selected: current
    Row {
      anchors.centerIn: parent
      spacing: FoamyStyle.gap
      FoamyIcon {
        anchors.verticalCenter: parent.verticalCenter
        width: FoamyStyle.iconSize
        height: width
        name: tabStop.iconName
        color: FoamyPalette.secondaryText
      }
      Text {
        anchors.verticalCenter: parent.verticalCenter
        text: tabStop.label + "  " + FoamyI18n.number(tabStop.count)
        textFormat: Text.PlainText
        color: FoamyPalette.text
        font.family: FoamyStyle.fontFamily
        font.pixelSize: FoamyStyle.font.body
      }
    }
  }

  component OverviewRow: FoamyCursorSurface {
    id: rowItem
    required property string key
    required property int index
    readonly property var row: (root.tab === "folders" ? root.folderMap : root.deviceMap)[key] || ({ related: [], progress: -1 })
    readonly property bool isExpanded: !!root.expanded[root.tab + ":" + key]
    readonly property bool isFolder: root.tab === "folders"
    x: FoamyStyle.gapLarge
    width: rows.width - FoamyStyle.gapLarge * 2
    height: rowContent.implicitHeight + FoamyStyle.rowPadding * 2
    hasCursor: cursor.at(index + root.firstRowStop)
    onHovered: cursor.hover(index + root.firstRowStop)
    onClicked: root.toggleRow(key)
    Accessible.role: Accessible.ListItem
    Accessible.name: String(row.label || "") + ", " + FoamyI18n.t(String(row.state || ""))
    Accessible.description: FoamyI18n.t(isExpanded ? "Collapse details" : "Expand details")

    Column {
      id: rowContent
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: FoamyStyle.rowPadding
      anchors.rightMargin: FoamyStyle.rowPadding
      spacing: FoamyStyle.gap

      RowLayout {
        width: parent.width
        spacing: FoamyStyle.rowPadding
        FoamyIcon {
          Layout.preferredWidth: FoamyStyle.iconSize
          Layout.preferredHeight: FoamyStyle.iconSize
          name: String(rowItem.row.icon || "folder")
          color: FoamyPalette.secondaryText
        }
        ColumnLayout {
          Layout.fillWidth: true
          Layout.minimumWidth: 0
          spacing: FoamyStyle.gapTiny
          Label {
            id: rowName
            Layout.fillWidth: true
            text: String(rowItem.row.label || "")
            elide: Text.ElideRight
          }
          Label {
            Layout.fillWidth: true
            text: String(rowItem.row.subtitle || "")
            elide: Text.ElideRight
            color: FoamyPalette.secondaryText
            font.pixelSize: FoamyStyle.font.caption
          }
        }
        Row {
          Layout.alignment: Qt.AlignVCenter
          spacing: FoamyStyle.gapSmall
          FoamyIcon {
            anchors.verticalCenter: parent.verticalCenter
            width: Style.space(12)
            height: width
            name: root.statusIcon(String(rowItem.row.state || ""))
            color: root.statusColor(String(rowItem.row.state || ""))
          }
          Text {
            anchors.verticalCenter: parent.verticalCenter
            text: FoamyI18n.t(String(rowItem.row.state || ""))
            textFormat: Text.PlainText
            color: root.statusColor(String(rowItem.row.state || ""))
            font.family: FoamyStyle.fontFamily
            font.pixelSize: FoamyStyle.font.bodySmall
          }
        }
        FoamyIcon {
          Layout.preferredWidth: Style.space(12)
          Layout.preferredHeight: Style.space(12)
          name: "chevron"
          rotation: rowItem.isExpanded ? (FoamyI18n.rightToLeft ? -90 : 90) : 0
          color: FoamyPalette.mutedText
          Behavior on rotation { NumberAnimation { duration: FoamyMotion.toggle } }
        }
      }

      Item {
        visible: rowItem.isExpanded
        width: parent.width
        height: visible ? details.implicitHeight : 0
        Column {
          id: details
          // Anchored (not x) so right-to-left layouts mirror the indent.
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.leftMargin: FoamyStyle.iconSize + FoamyStyle.rowPadding
          spacing: FoamyStyle.gap
          bottomPadding: FoamyStyle.gapSmall
          Label {
            visible: rowName.truncated
            width: parent.width
            text: String(rowItem.row.label || "")
            wrapMode: Text.WrapAnywhere
          }
          Label {
            visible: text !== ""
            width: parent.width
            text: String(rowItem.row.detail || "")
            wrapMode: Text.WrapAnywhere
            color: rowItem.row.state === "Error" || rowItem.row.state === "Out of sync" || rowItem.row.state === "Conflicts"
              ? Commons.Color.urgent : FoamyPalette.secondaryText
            font.pixelSize: FoamyStyle.font.bodySmall
          }
          RowLayout {
            visible: Number(rowItem.row.progress) >= 0
            width: parent.width
            spacing: FoamyStyle.gap
            Rectangle {
              Layout.fillWidth: true
              implicitHeight: Style.space(3)
              radius: FoamyStyle.pillRadius(height)
              color: FoamyPalette.fieldFill
              Rectangle {
                height: parent.height
                radius: parent.radius
                width: parent.width * Math.max(0, Number(rowItem.row.progress)) / 100
                color: Commons.Color.accent
              }
            }
            Text {
              text: FoamyI18n.t("{percent}%", { percent: FoamyI18n.number(Math.max(0, Number(rowItem.row.progress))) })
              textFormat: Text.PlainText
              color: Commons.Color.accent
              font.family: FoamyStyle.fontFamily
              font.pixelSize: FoamyStyle.font.bodySmall
            }
          }
          Repeater {
            model: rowItem.isExpanded ? rowItem.row.related : []
            delegate: RowLayout {
              id: related
              required property var modelData
              width: parent ? parent.width : 0
              spacing: FoamyStyle.gap
              FoamyIcon {
                Layout.preferredWidth: Style.space(13)
                Layout.preferredHeight: Style.space(13)
                name: rowItem.isFolder ? "devices" : "folder"
                color: FoamyPalette.secondaryText
              }
              Label {
                Layout.fillWidth: true
                text: String(related.modelData.label || "")
                elide: Text.ElideRight
                font.pixelSize: FoamyStyle.font.bodySmall
              }
              Label {
                text: FoamyI18n.t(String(related.modelData.state || ""))
                color: root.statusColor(String(related.modelData.state || ""))
                font.pixelSize: FoamyStyle.font.bodySmall
              }
            }
          }
          FoamyAction {
            visible: rowItem.isFolder
            anchors.right: parent.right
            iconName: "external-link"
            label: FoamyI18n.t("Open folder")
            tooltipText: FoamyI18n.t("Open in the file manager") + " · O"
            foreground: FoamyPalette.secondaryText
            onClicked: if (rowItem.row.folder) root.openFolderRequested(rowItem.row.folder)
          }
        }
      }
    }
  }

  component SettingsFolderRow: RowLayout {
    id: folderRow
    required property string key
    readonly property var folder: root.settingsMap[key] || ({})
    width: parent ? parent.width : 0
    spacing: FoamyStyle.rowPadding
    Rectangle {
      Layout.preferredWidth: 2
      Layout.preferredHeight: FoamyStyle.controlHeight
      radius: FoamyStyle.pillRadius(width)
      color: root.folderStateColor(folderRow.folder)
    }
    ColumnLayout {
      Layout.fillWidth: true
      Layout.minimumWidth: 0
      spacing: FoamyStyle.gapTiny
      Label {
        Layout.fillWidth: true
        text: String(folderRow.folder.label || "")
        elide: Text.ElideRight
        font.pixelSize: FoamyStyle.font.title
      }
      Label {
        Layout.fillWidth: true
        text: String(folderRow.folder.path || "")
        elide: Text.ElideMiddle
        color: FoamyPalette.secondaryText
        font.pixelSize: FoamyStyle.font.bodySmall
      }
      Label {
        Layout.fillWidth: true
        text: folderRow.folder.problem || folderRow.folder.conflict
          ? PanelModel.folderMeta(folderRow.folder, FoamyI18n.t, FoamyI18n.tn)
          : FoamyI18n.t(PanelModel.compactFolderState(folderRow.folder, root.online,
              !!root.service && root.service.serviceAvailable && !root.service.serviceActive, false))
        wrapMode: Text.WordWrap
        color: root.folderStateColor(folderRow.folder)
        font.pixelSize: FoamyStyle.font.caption
      }
    }
    FoamyAction {
      iconName: folderRow.folder.paused ? "play" : "pause"
      focusable: true
      tooltipText: FoamyI18n.t(folderRow.folder.paused ? "Resume synchronization for {name}" : "Pause synchronization for {name}", { name: String(folderRow.folder.label || "") })
      actionEnabled: root.canManage
      foreground: FoamyPalette.secondaryText
      onClicked: root.service.setFolderPaused(folderRow.folder.id, !folderRow.folder.paused)
    }
    FoamyAction {
      iconName: "unlink"
      focusable: true
      destructive: true
      tooltipText: FoamyI18n.t("Unlink {name} from Syncthing; keep local files", { name: String(folderRow.folder.label || "") })
      actionEnabled: root.canManage
      foreground: FoamyPalette.secondaryText
      onClicked: root.requestUnlink(folderRow.folder)
    }
    FoamyAction {
      iconName: "folder"
      focusable: true
      tooltipText: FoamyI18n.t("Open {path}\nFolder ID: {id}", { path: String(folderRow.folder.path || ""), id: String(folderRow.folder.id || "") })
      actionEnabled: String(folderRow.folder.path || "") !== ""
      foreground: FoamyPalette.secondaryText
      onClicked: root.openFolderRequested(folderRow.folder)
    }
  }
}
