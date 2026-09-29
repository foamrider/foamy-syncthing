pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import qs.Commons
import "../models/OverviewModel.js" as OverviewModel

ColumnLayout {
  id: root
  required property var controller
  signal closeRequested()
  property string tab: "folders"
  property var expandedFolders: ({})
  property var expandedDevices: ({})
  readonly property var rows: OverviewModel.rows(controller.syncthing, controller.folderRows,
    controller.tr, controller.formatBytes, controller.formatCount)
  readonly property var currentRows: tab === "folders" ? rows.folders : rows.devices
  spacing: Style.space(8)

  function scrollToTop() { flick.contentY = 0 }
  function scrollRows(dy) { flick.contentY = Math.max(0, Math.min(flick.contentHeight - flick.height, flick.contentY + dy * Style.space(40))) }
  function focusFirstAction() { folderTab.forceActiveFocus(Qt.TabFocusReason) }
  function setTab(value) { tab = value; scrollToTop() }
  function toggleRow(id) {
    var next = Object.assign({}, tab === "folders" ? expandedFolders : expandedDevices)
    next[id] = !next[id]
    if (tab === "folders") expandedFolders = next
    else expandedDevices = next
  }
  function statusColor(state) {
    if (state === "Error") return controller.urgent
    if (state === "Syncing" || state === "Scanning" || state === "Checking") return Color.accent
    if (state === "Connected" || state === "Up to date") return controller.success
    if (state === "Paused" || state === "Waiting to sync") return controller.warning
    return controller.panelMuted
  }
  function statusIcon(state) {
    if (state === "Error") return "alert"
    if (state === "Connected" || state === "Up to date") return "check"
    if (state === "Paused") return "pause"
    if (state === "Disconnected" || state === "Unavailable") return "disconnected"
    return "sync"
  }
  component Label: Text {
    textFormat: Text.PlainText
    color: root.controller.foreground
    font.family: "sans-serif"
    font.pixelSize: Style.space(13)
  }
  component TabButton: Controls.AbstractButton {
    id: button
    required property string view
    required property string iconName
    checked: root.tab === view
    implicitHeight: Style.space(36)
    onClicked: root.setTab(view)
    Keys.onEscapePressed: root.closeRequested()
    Keys.onLeftPressed: { root.setTab("folders"); folderTab.forceActiveFocus(Qt.TabFocusReason) }
    Keys.onRightPressed: { root.setTab("devices"); deviceTab.forceActiveFocus(Qt.TabFocusReason) }
    background: Rectangle {
      radius: Style.space(6)
      color: button.checked || button.hovered ? root.controller.panelFill : "transparent"
      border.width: button.visualFocus ? 1 : 0
      border.color: Color.accent
    }
    contentItem: Row {
      spacing: Style.space(7)
      padding: 0
      // Center the complete icon-and-label group within each equal-width tab.
      leftPadding: Math.max(0, (button.width - tabIcon.width - tabLabel.implicitWidth - spacing) / 2)
      SyncthingIcon { id: tabIcon; anchors.verticalCenter: parent.verticalCenter; width: Style.space(16); height: width; name: button.iconName; color: root.controller.panelMuted }
      Label { id: tabLabel; anchors.verticalCenter: parent.verticalCenter; text: button.text; font.pixelSize: Style.space(12) }
    }
  }
  RowLayout {
    Layout.fillWidth: true
    spacing: Style.space(4)
    TabButton { id: folderTab; Layout.fillWidth: true; view: "folders"; iconName: "folder"; text: root.controller.tr("Folders") + "  " + root.rows.folders.length }
    TabButton { id: deviceTab; Layout.fillWidth: true; view: "devices"; iconName: "devices"; text: root.controller.tr("Devices") + "  " + root.rows.devices.length }
  }
  Flickable {
    id: flick
    Layout.fillWidth: true
    Layout.fillHeight: true
    Layout.minimumHeight: 0
    Layout.maximumHeight: implicitHeight
    implicitHeight: contents.implicitHeight
    contentWidth: width
    contentHeight: contents.implicitHeight
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    flickableDirection: Flickable.VerticalFlick
    interactive: contentHeight > height
    Controls.ScrollBar.vertical: Controls.ScrollBar { policy: Controls.ScrollBar.AsNeeded }
    Column {
      id: contents
      width: flick.width
      Label {
        visible: root.currentRows.length === 0
        width: parent.width
        padding: Style.space(12)
        text: !root.controller.syncthing || !root.controller.syncthing.online
          ? root.controller.tr("Current status is unavailable.")
          : root.controller.tr(root.tab === "folders" ? "No shared folders" : "No remote devices configured.")
        wrapMode: Text.WordWrap
        color: root.controller.panelMuted
      }
      Repeater {
        model: root.currentRows
        delegate: Column {
          id: row
          required property var modelData
          required property int index
          readonly property bool expanded: !!(root.tab === "folders" ? root.expandedFolders : root.expandedDevices)[modelData.id]
          width: contents.width
          Rectangle { visible: row.index > 0; width: parent.width; height: visible ? 1 : 0; color: root.controller.panelOutline; opacity: 0.5 }
          Controls.AbstractButton {
            id: rowButton
            width: parent.width
            implicitHeight: Math.max(Style.space(58), nameColumn.implicitHeight + Style.space(20))
            Accessible.name: row.modelData.label + ", " + root.controller.tr(row.modelData.state)
            Accessible.description: root.controller.tr(row.expanded ? "Collapse details" : "Expand details")
            onClicked: root.toggleRow(row.modelData.id)
            Keys.onEscapePressed: root.closeRequested()
            onActiveFocusChanged: {
              if (!activeFocus) return
              if (row.y < flick.contentY) flick.contentY = row.y
              else if (row.y + height > flick.contentY + flick.height) flick.contentY = row.y + height - flick.height
            }
            background: Rectangle { radius: Style.space(6); color: rowButton.hovered || rowButton.activeFocus ? root.controller.panelFill : "transparent"; border.width: rowButton.visualFocus ? 1 : 0; border.color: Color.accent }
            contentItem: Item {
              SyncthingIcon { id: kind; x: Style.space(2); anchors.verticalCenter: parent.verticalCenter; width: Style.space(16); height: width; name: row.modelData.icon; color: root.controller.panelMuted }
              Column {
                id: nameColumn
                anchors.left: kind.right; anchors.leftMargin: Style.space(10)
                anchors.right: status.left; anchors.rightMargin: Style.space(8)
                anchors.verticalCenter: parent.verticalCenter
                spacing: Style.space(3)
                Label { id: rowName; width: parent.width; text: row.modelData.label; elide: Text.ElideRight }
                Label { width: parent.width; text: row.modelData.subtitle; elide: Text.ElideRight; font.pixelSize: Style.space(11); color: root.controller.panelMuted }
              }
              Row {
                id: status
                anchors.right: arrow.left; anchors.rightMargin: Style.space(8)
                anchors.verticalCenter: parent.verticalCenter
                spacing: Style.space(4)
                SyncthingIcon { anchors.verticalCenter: parent.verticalCenter; width: Style.space(12); height: width; name: root.statusIcon(row.modelData.state); color: root.statusColor(row.modelData.state) }
                Label { text: root.controller.tr(row.modelData.state); font.pixelSize: Style.space(11); color: root.statusColor(row.modelData.state) }
              }
              SyncthingIcon { id: arrow; anchors.right: parent.right; anchors.rightMargin: Style.space(4); anchors.verticalCenter: parent.verticalCenter; width: Style.space(12); height: width; name: "chevron"; rotation: row.expanded ? 90 : 0; color: root.controller.panelMuted }
            }
          }
          Column {
            id: details
            visible: row.expanded
            topPadding: Style.space(8)
            x: Style.space(28)
            width: parent.width - x - Style.space(4)
            spacing: Style.space(8)
            Label { visible: rowName.truncated; width: parent.width; text: row.modelData.label; wrapMode: Text.WrapAnywhere }
            Label { visible: text !== ""; width: parent.width; text: row.modelData.detail; wrapMode: Text.WrapAnywhere; font.pixelSize: Style.space(11); color: row.modelData.state === "Error" ? root.controller.urgent : root.controller.panelMuted }
            RowLayout {
              visible: row.modelData.progress >= 0
              width: parent.width
              Rectangle {
                Layout.fillWidth: true; implicitHeight: Style.space(3); color: root.controller.panelFill
                Rectangle { height: parent.height; width: parent.width * Math.max(0, row.modelData.progress) / 100; color: Color.accent }
              }
              Label { text: row.modelData.progress + " %"; font.pixelSize: Style.space(11); color: Color.accent }
            }
            Repeater {
              model: row.modelData.related
              delegate: RowLayout {
                id: related
                required property var modelData
                width: details.width
                SyncthingIcon { Layout.preferredWidth: Style.space(13); Layout.preferredHeight: Style.space(13); name: root.tab === "folders" ? "devices" : "folder"; color: root.controller.panelMuted }
                Label { Layout.fillWidth: true; text: related.modelData.label; wrapMode: Text.WrapAnywhere; font.pixelSize: Style.space(11) }
                Label { text: root.controller.tr(related.modelData.state); color: root.statusColor(related.modelData.state); font.pixelSize: Style.space(11) }
              }
            }
            Controls.AbstractButton {
              id: openFolder
              visible: root.tab === "folders"
              anchors.right: parent.right
              text: root.controller.tr("Open folder")
              implicitHeight: Style.space(30)
              leftPadding: Style.space(8)
              rightPadding: Style.space(8)
              implicitWidth: folderAction.implicitWidth + leftPadding + rightPadding
              onClicked: root.controller.openFolder(row.modelData.folder)
              Keys.onEscapePressed: root.closeRequested()
              background: Rectangle { radius: Style.space(5); color: root.controller.panelFill; border.width: openFolder.visualFocus ? 1 : 0; border.color: Color.accent }
              contentItem: Row {
                id: folderAction
                spacing: Style.space(6)
                Label { anchors.verticalCenter: parent.verticalCenter; text: openFolder.text; font.pixelSize: Style.space(11) }
                SyncthingIcon { anchors.verticalCenter: parent.verticalCenter; width: Style.space(14); height: width; name: "external"; color: root.controller.foreground }
              }
            }
            Item { width: 1; height: Style.space(4) }
          }
        }
      }
    }
  }
}
