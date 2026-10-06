pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as Controls
import qs.Commons
import qs.Ui

ColumnLayout {
  id: root
  function tr(key, values) { return controller.tr(key, values) }
  required property var controller
  property PanelKeyCatcher keyTarget
  readonly property var syncthing: controller.syncthing
  readonly property bool popupOpen: offerSelector.popupOpen || languagePicker.popupOpen || minimumPicker.popupOpen
  readonly property bool canManage: syncthing && syncthing.online && !syncthing.folderMutationBusy
  spacing: Style.space(12)

  function scrollToTop() { folderFlick.contentY = 0 }
  function scrollFolders(dy) {
    folderFlick.contentY = Math.max(0, Math.min(folderFlick.contentHeight - folderFlick.height,
      folderFlick.contentY + dy * Style.space(40)))
  }
  // Keyboard focus must keep its folder's actions inside the scroll viewport.
  function ensureFolderVisible(row) {
    if (row.y < folderFlick.contentY) folderFlick.contentY = row.y
    else if (row.y + row.height > folderFlick.contentY + folderFlick.height)
      folderFlick.contentY = row.y + row.height - folderFlick.height
  }
  function closePopups() { offerSelector.close(); languagePicker.close(); minimumPicker.close() }
  function focusFirstAction() {
    if (addAction.enabled) addAction.forceActiveFocus(Qt.TabFocusReason)
  }

  component Label: Text {
    textFormat: Text.PlainText
    color: root.controller.foreground
    font.family: "sans-serif"
    font.pixelSize: Style.space(12)
  }
  component Action: SyncthingAction {
    foreground: root.controller.panelMuted
    keyTarget: root.keyTarget
    implicitWidth: Style.space(28)
    implicitHeight: Style.space(28)
  }

  Column {
    Layout.fillWidth: true
    spacing: Style.space(12)
    Label { text: root.tr("Plugin preferences"); font.pixelSize: Style.space(14) }
    SyncthingDropdown {
      id: languagePicker
      width: parent.width
      label: root.tr("Language")
      fontFamily: "sans-serif"
      value: root.controller.preferences.languageMode
      options: [
        {value: "system", label: root.tr("Default (system language)")},
        {value: "nb", label: "Norsk bokmål"},
        {value: "en", label: "English"}
      ]
      enabled: !root.controller.preferencesSaving
      onChanged: function(value) { root.controller.savePreference("language", value) }
    }
    SyncthingDropdown {
      id: minimumPicker
      width: parent.width
      label: root.tr("Minimum connected devices (this host included)")
      fontFamily: "sans-serif"
      value: String(root.controller.preferences.minimumConnectedDevices)
      options: {
        var rows = [{value:"0",label:root.tr("No warning")}]
        var maximum = Math.min(100, Math.max(5, root.controller.deviceCounts.total, root.controller.preferences.minimumConnectedDevices))
        for (var i = 1; i <= maximum; ++i) rows.push({value:String(i),label:String(i)})
        return rows
      }
      enabled: !root.controller.preferencesSaving
      onChanged: function(value) { root.controller.savePreference("minimumConnectedDevices", Number(value)) }
    }
    Label {
      visible: text !== ""; width: parent.width
      text: root.tr(root.controller.preferencesError)
      color: root.controller.urgent; wrapMode: Text.WordWrap
    }
  }

  Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: root.controller.panelOutline }

  RowLayout {
    Layout.fillWidth: true
    Label {
      Layout.fillWidth: true
      text: root.tr("Shared folders")
      font.pixelSize: Style.space(14)
    }
    Action {
      id: addAction
      iconName: "plus"
      tooltipText: root.tr("Add folder")
      enabled: root.canManage
      onClicked: root.controller.openAddFolder()
    }
  }

  Flickable {
    id: folderFlick
    Layout.fillWidth: true
    Layout.fillHeight: true
    Layout.minimumHeight: 0
    Layout.maximumHeight: implicitHeight
    implicitHeight: folderList.implicitHeight
    contentWidth: width
    contentHeight: folderList.implicitHeight
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    flickableDirection: Flickable.VerticalFlick
    interactive: contentHeight > height
    Controls.ScrollBar.vertical: Controls.ScrollBar { policy: Controls.ScrollBar.AsNeeded }
    Column {
      id: folderList
      width: folderFlick.width
      spacing: 0
      Label {
        visible: root.controller.folderRows.length === 0
        width: parent.width
        text: root.syncthing && root.syncthing.online
          ? root.tr("No shared folders yet. Add an existing directory to get started.")
          : root.tr("Connect to Syncthing to manage folders.")
        color: root.controller.panelMuted
        wrapMode: Text.WordWrap
      }
      Repeater {
        model: root.controller.folderRows
        delegate: Item {
          id: folderRow
          required property var modelData
          required property int index
          width: parent.width
          height: folderContent.implicitHeight + Style.space(24)
          Rectangle {
            visible: folderRow.index < root.controller.folderRows.length - 1
            anchors.bottom: parent.bottom
            x: Style.space(7)
            width: parent.width - Style.space(14); height: 1
            color: Qt.rgba(root.controller.foreground.r, root.controller.foreground.g,
              root.controller.foreground.b, 0.08)
          }
          RowLayout {
            anchors.left: parent.left; anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            anchors.leftMargin: Style.space(2); anchors.rightMargin: Style.space(2)
            spacing: Style.space(12)
            Rectangle {
              implicitWidth: Style.space(2); implicitHeight: Style.space(32); radius: width / 2
              color: root.controller.folderStateColor(folderRow.modelData)
            }
            ColumnLayout {
              id: folderContent
              Layout.fillWidth: true
              spacing: Style.space(3)
              Label {
                Layout.fillWidth: true
                text: folderRow.modelData.label
                font.pixelSize: Style.space(14)
                elide: Text.ElideRight
              }
              Label {
                Layout.fillWidth: true
                text: folderRow.modelData.path
                color: root.controller.panelMuted
                font.pixelSize: Style.space(11)
                elide: Text.ElideMiddle
              }
              Label {
                Layout.fillWidth: true
                text: folderRow.modelData.problem ? root.controller.folderMeta(folderRow.modelData)
                  : root.tr(root.controller.compactFolderState(folderRow.modelData))
                color: root.controller.folderStateColor(folderRow.modelData)
                font.pixelSize: Style.space(10)
                wrapMode: Text.WordWrap
              }
            }
            RowLayout {
              spacing: Style.space(2)
              Action {
                iconName: folderRow.modelData.paused ? "play" : "pause"
                tooltipText: root.tr(folderRow.modelData.paused ? "Resume synchronization for {name}" : "Pause synchronization for {name}", {name: folderRow.modelData.label})
                enabled: root.canManage
                onActiveFocusChanged: if (activeFocus) root.ensureFolderVisible(folderRow)
                onClicked: root.syncthing.setFolderPaused(folderRow.modelData.id, !folderRow.modelData.paused)
              }
              Action {
                iconName: "unlink"
                hoverForeground: root.controller.urgent
                enabled: root.canManage
                tooltipText: root.tr("Unlink {name} from Syncthing; keep local files", {name: folderRow.modelData.label})
                onActiveFocusChanged: if (activeFocus) root.ensureFolderVisible(folderRow)
                onClicked: root.controller.requestForget(folderRow.modelData)
              }
              Action {
                iconName: "folder"
                tooltipText: root.tr("Open {path}\nFolder ID: {id}", {path: folderRow.modelData.path, id: folderRow.modelData.id})
                enabled: String(folderRow.modelData.path || "") !== ""
                onActiveFocusChanged: if (activeFocus) root.ensureFolderVisible(folderRow)
                onClicked: root.controller.openFolder(folderRow.modelData)
              }
            }
          }
        }
      }
    }
  }

  Column {
    visible: root.controller.pendingOfferRows.length > 0
    Layout.fillWidth: true
    spacing: Style.space(8)
    Label { text: root.tr("Incoming shares"); font.pixelSize: Style.space(14) }
    RowLayout {
      width: parent.width
      spacing: Style.space(8)
      ToggleDropdown {
        id: offerSelector
        Layout.fillWidth: true
        showLabel: false
        value: root.controller.selectedPendingOffer
        options: root.controller.pendingOfferRows
        foreground: root.controller.foreground
        fontFamily: "sans-serif"
        onChanged: function(value) { root.controller.selectedPendingOffer = value }
      }
      Action {
        iconName: "check"
        tooltipText: root.tr("Accept incoming share")
        enabled: root.canManage && root.controller.selectedPendingOffer !== ""
        onClicked: root.controller.acceptPendingFolderOffer(root.controller.selectedPendingOffer)
      }
    }
  }

}
