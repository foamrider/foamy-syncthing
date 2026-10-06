pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import qs.Commons
import qs.Ui

SyncthingPopup {
  id: root
  function tr(key, values) { return controller.tr(key, values) }
  property var controller
  readonly property var syncthing: controller.syncthing
  readonly property color foreground: controller.foreground
  readonly property color muted: controller.panelMuted
  property alias addPathText: addForm.pathText
  property alias addLabelText: addForm.labelText
  property alias addIdText: addForm.idText
  property alias selectedDeviceIds: addForm.selectedDeviceIds
  property alias pendingFolderValue: addForm.pendingFolderValue

  function resetAddForm() { addForm.reset() }
  function closeTransientPopups() {
    settings.closePopups()
    addForm.closePopups()
  }
  function focusAddPath() { addForm.focusPath() }
  function focusPanel() { keyCatcher.forceActiveFocus() }
  function scrollToTop() { overview.scrollToTop(); settings.scrollToTop() }

  focusTarget: keyCatcher
  padding: 0
  borderSpec: Border.flat(controller.panelOutline, 1)
  contentWidth: fittedContentWidth(Style.space(380))
  contentHeight: fittedContentHeight(header.height + body.implicitHeight
    + footer.height, Style.space(900))

  component Label: Text {
    textFormat: Text.PlainText
    color: root.foreground
    font.family: "sans-serif"
    font.pixelSize: Style.space(13)
  }

  PanelKeyCatcher {
    id: keyCatcher
    anchors.fill: parent
    blocked: root.controller.addOpen || settings.popupOpen
    onCloseRequested: {
      if (root.controller.forgetConfirmOpen) {
        root.controller.forgetConfirmOpen = false
        root.controller.forgetFolderId = ""
      } else if (root.controller.addOpen) root.controller.closeAddFolder()
      else if (root.controller.moreOpen) root.controller.hideSettings()
      else root.controller.close()
    }
    onTabRequested: function(direction) {
      if (!root.controller.moreOpen) {
        overview.focusFirstAction()
        return
      }
      // Enter the settings controls; subsequent Tab presses use Qt's focus chain.
      settings.focusFirstAction()
    }
    onMoveRequested: function(dx, dy) {
      if (root.controller.forgetConfirmOpen && (dx !== 0 || dy !== 0)) {
        forgetDialog.selectedIndex = forgetDialog.selectedIndex === 0 ? 1 : 0
      } else if (dy !== 0) {
        if (root.controller.moreOpen) settings.scrollFolders(dy)
        else overview.scrollRows(dy)
      }
    }
    onReturnRequested: {
      if (!root.controller.forgetConfirmOpen) return
      if (forgetDialog.selectedIndex === 0) {
        root.controller.forgetConfirmOpen = false
        root.controller.forgetFolderId = ""
      } else root.controller.confirmForget()
    }
    onTextKey: function(text) {
      if (root.controller.forgetConfirmOpen) return
      var key = text.toLowerCase()
      if (key === "r" && root.syncthing) root.syncthing.refresh()
      else if (key === "w") root.controller.openWebUi()
      else if (key === "p") root.controller.toggleSyncing()
      else if (key === "m") root.controller.moreOpen
        ? root.controller.hideSettings() : root.controller.showSettings()
      else if (key === "q") root.controller.close()
    }
  }

  Item {
    id: header
    anchors.top: parent.top
    width: parent.width
    height: Style.space(root.controller.moreOpen ? 88 : 58)
    Item {
      anchors.fill: parent
      clip: true
      // Qt clamps a short Rectangle's corners; crop a taller background instead.
      Rectangle {
        width: parent.width
        height: Math.max(parent.height, topLeftRadius * 2)
        topLeftRadius: Math.max(0, root.cornerRadius - Border.top(root.borderSpec))
        topRightRadius: topLeftRadius
        color: Qt.tint(Color.popups.background, Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.09))
      }
    }
    Row {
      anchors.left: parent.left
      anchors.leftMargin: Style.space(20)
      y: Style.space(13)
      height: Style.space(32)
      spacing: Style.space(8)
      SyncthingIcon {
        visible: !root.controller.moreOpen
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(16); height: width
        name: "sync"; color: Color.accent
      }
      SyncthingAction {
        keyTarget: keyCatcher
        visible: root.controller.moreOpen
        iconName: "back"; tooltipText: root.tr("Back to overview · Esc")
        foreground: root.muted
        enabled: !root.controller.forgetConfirmOpen
        onClicked: root.controller.addOpen ? root.controller.closeAddFolder() : root.controller.hideSettings()
      }
      Label {
        anchors.verticalCenter: parent.verticalCenter
        text: root.controller.moreOpen ? (root.controller.addOpen ? root.tr("Add folder") : root.tr("Settings")) : root.tr("Syncthing")
        font.pixelSize: Style.space(12)
        color: root.muted
      }
    }
    Label {
      visible: root.controller.moreOpen
      x: Style.space(20); y: Style.space(56)
      width: parent.width - Style.space(40)
      text: root.controller.addOpen ? root.tr("Choose a directory and devices to share with.")
        : root.tr("Manage shared folders and plugin preferences.")
      color: root.muted
      font.pixelSize: Style.space(11)
      elide: Text.ElideRight
    }
    SyncthingAction {
      keyTarget: keyCatcher
      anchors.right: parent.right
      anchors.rightMargin: Style.space(16)
      anchors.verticalCenter: parent.verticalCenter
      visible: !root.controller.moreOpen
      iconName: "settings"; tooltipText: root.tr("Settings · M")
      foreground: root.muted
      onClicked: root.controller.showSettings()
    }
  }

  // Keep headings, preferences, notices and footer fixed; only folder rows scroll.
  ColumnLayout {
    id: body
    anchors.top: header.bottom
    anchors.bottom: footer.top
    width: parent.width
    spacing: 0
    Item {
      visible: !root.controller.moreOpen
      Layout.fillWidth: true
      implicitHeight: hero.implicitHeight + Style.space(30)
      Rectangle {
        anchors.fill: parent
        color: Qt.tint(Color.popups.background,
          Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.09))
      }
      Column {
        id: hero
        x: Style.space(20); y: Style.space(8)
        width: parent.width - Style.space(40)
        spacing: Style.space(12)
        Item {
          width: parent.width
          height: Math.max(summaryText.implicitHeight, percentage.visible ? percentage.height : 0)
          Column {
            id: summaryText
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width - (percentage.visible ? percentage.width + Style.space(12) : 0)
            spacing: Style.space(5)
            Label {
              width: parent.width
              text: root.tr(root.controller.compactSummary.title)
              font.pixelSize: Style.space(23)
              wrapMode: Text.WordWrap
            }
            Label {
              width: parent.width
              text: root.tr(root.controller.compactSummary.detail)
              color: root.muted
              font.pixelSize: Style.space(11)
              wrapMode: Text.WordWrap
            }
          }
          SyncthingProgress {
            id: percentage
            language: root.controller.language
            width: Style.space(64); height: width
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            progress: root.controller.compactSummary.progress
            foreground: root.foreground
            accentColor: Color.accent
            trackColor: root.controller.panelFill
            successColor: root.controller.success
          }
        }
        Label {
          width: parent.width
          text: root.tr("{count} files · {size}", {count: root.controller.formatCount(root.controller.trackedFiles),
            size: root.controller.formatBytes(root.controller.trackedBytes)})
          color: root.muted
          font.pixelSize: Style.space(12)
          visible: root.syncthing && root.syncthing.online && root.controller.folderRows.length > 0
        }
      }
    }
    ColumnLayout {
      Layout.fillWidth: true
      Layout.fillHeight: true
      Layout.minimumHeight: 0
      Layout.leftMargin: Style.space(20)
      Layout.rightMargin: Style.space(20)
      Layout.topMargin: Style.space(12)
      Layout.bottomMargin: Style.space(10)
      spacing: Style.space(10)
      Label {
        visible: text !== ""; Layout.fillWidth: true
        text: root.tr(root.controller.visibleWarning); color: root.controller.warning
        font.pixelSize: Style.space(12); wrapMode: Text.WordWrap
      }
      Label {
        visible: text !== ""; Layout.fillWidth: true
        text: root.tr(root.controller.visibleError); color: root.controller.urgent
        font.pixelSize: Style.space(12); wrapMode: Text.WordWrap
      }
      Label {
        visible: text !== ""; Layout.fillWidth: true
        text: root.tr(root.controller.displayedNotice)
        opacity: root.controller.noticeShown ? 1 : 0
        color: root.controller.success
        font.pixelSize: Style.space(12); wrapMode: Text.WordWrap
        Behavior on opacity { NumberAnimation { duration: 350 } }
      }
      SyncthingOverview {
        id: overview
        visible: !root.controller.moreOpen
        Layout.fillWidth: true
        Layout.fillHeight: true
        Layout.minimumHeight: 0
        controller: root.controller
        onCloseRequested: keyCatcher.closeRequested()
      }
      AddFolderForm {
        id: addForm
        visible: root.controller.moreOpen && root.controller.addOpen
        Layout.fillWidth: true
        controller: root.controller; syncthing: root.syncthing
        folderPickerRunning: root.controller.folderPickerRunning
        foreground: root.foreground; dim: root.muted
        urgent: root.controller.urgent; warning: root.controller.warning
        success: root.controller.success; fontFamily: "sans-serif"
      }
      SyncthingSettings {
        id: settings
        visible: root.controller.moreOpen && !root.controller.addOpen
        Layout.fillWidth: true
        Layout.fillHeight: true
        Layout.minimumHeight: 0
        controller: root.controller
        keyTarget: keyCatcher
      }
    }
  }

  Item {
    id: footer
    anchors.bottom: parent.bottom
    x: Style.space(20)
    width: parent.width - Style.space(40)
    height: Style.space(52)
    Rectangle { width: parent.width; height: 1; color: root.controller.panelOutline }
    Row {
      anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(6)
      Rectangle {
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(5); height: width; radius: width / 2
        color: !root.syncthing || !root.syncthing.serviceActive || !root.syncthing.online ? root.controller.urgent
          : root.controller.hasProblems ? root.controller.urgent
          : root.controller.hasTooFewDevices ? root.controller.warning : root.controller.success
      }
      Label {
        text: root.syncthing && root.syncthing.online && root.syncthing.serviceActive
          ? root.tr("{connected} / {total} devices online", root.controller.deviceCounts)
          : root.syncthing && root.syncthing.installationState === "missing" ? root.tr("Syncthing is not installed")
          : root.syncthing && !root.syncthing.serviceActive ? root.tr("Service is not running")
          : root.tr("Syncthing unavailable")
        font.pixelSize: Style.space(11); color: root.muted
      }
    }
    Row {
      anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(4)
      SyncthingAction {
        keyTarget: keyCatcher
        iconName: root.syncthing && root.syncthing.canInstall ? "download"
          : root.syncthing && root.syncthing.serviceActive ? "pause" : "play"
        tooltipText: root.syncthing && root.syncthing.canInstall ? root.tr("Install Syncthing")
          : root.tr(root.controller.toggleHint) + " · P"
        foreground: root.muted
        enabled: root.syncthing && (root.syncthing.canInstall || root.syncthing.canControlService)
          && !root.syncthing.serviceActionRunning && !root.syncthing.folderMutationBusy
        onClicked: root.syncthing.canInstall ? root.controller.installationAction() : root.controller.toggleSyncing()
      }
      SyncthingAction {
        keyTarget: keyCatcher
        iconName: "globe"; tooltipText: root.tr("Open Syncthing Web UI · W")
        foreground: root.muted
        enabled: root.syncthing && root.syncthing.online
        onClicked: root.controller.openWebUi()
      }
    }
  }
  CompactConfirmDialog {
    id: forgetDialog
    anchors.fill: parent
    opened: root.controller.forgetConfirmOpen
    onOpenedChanged: if (opened) selectedIndex = 0
    z: 10
    message: {
      var folder = root.controller.folderById(root.controller.forgetFolderId)
      return folder
        ? root.tr("Unlink {name} ({id}) from Syncthing?", {name: folder.label, id: folder.id}) + "\n\n"
          + root.tr("This removes its Syncthing configuration. Local files will be kept.") + " "
          + (folder.markerName === ".stfolder" ? root.tr("Syncthing may remove its internal .stfolder marker.") + " " : "")
          + root.tr("Keep this folder ID to rejoin the same remote folder.")
        : root.tr("Unlink this folder from Syncthing? Local files will be kept.")
    }
    confirmText: root.tr("Unlink")
    cancelText: root.tr("Cancel")
    background: Color.popups.background
    foreground: root.controller.foreground
    selectedText: root.controller.urgent
    fontFamily: "sans-serif"
    onCanceled: {
      root.controller.forgetConfirmOpen = false
      root.controller.forgetFolderId = ""
    }
    onConfirmed: root.controller.confirmForget()
  }
}
