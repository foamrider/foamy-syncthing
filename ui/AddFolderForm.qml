pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import qs.Commons
import qs.Ui

Column {
  id: root
  function tr(key, values) { return controller.tr(key, values) }
  property var controller
  property var syncthing
  property bool folderPickerRunning: false
  property color foreground: Color.foreground
  property color dim: Qt.darker(foreground, 1.5)
  property color urgent: Color.urgent
  property color warning: "#ebcb8b"
  property color success: "#a3be8c"
  property string fontFamily: "sans-serif"
  property alias pathText: addPathField.text
  property alias labelText: addLabelField.text
  property alias idText: addIdField.text
  property var selectedDeviceIds: []
  property alias pendingFolderValue: pendingFolderPicker.value
  readonly property bool busy: syncthing && syncthing.folderMutationBusy
  readonly property var devices: controller.deviceOptions()
  width: parent ? parent.width : implicitWidth
  spacing: Style.space(14)

  function reset() {
    addPathField.text = ""
    addLabelField.text = ""
    addIdField.text = ""
    selectedDeviceIds = []
    pendingFolderPicker.value = ""
    closePopups()
  }
  function closePopups() { pendingFolderPicker.close() }
  function focusPath() { addPathField.forceActiveFocus() }
  function toggleDevice(value) {
    if (busy) return
    // Assign a new list so selection and pending-offer bindings update together.
    var next = selectedDeviceIds.slice()
    var index = next.indexOf(value)
    if (index < 0) next.push(value)
    else next.splice(index, 1)
    selectedDeviceIds = next
  }
  Keys.onEscapePressed: controller.closeAddFolder()

  component Label: Text {
    textFormat: Text.PlainText
    font.family: root.fontFamily
    font.pixelSize: Style.space(12)
    color: root.foreground
  }
  component Hint: Label {
    font.pixelSize: Style.space(11)
    color: root.dim
    wrapMode: Text.WordWrap
  }
  component Field: Controls.TextField {
    id: field
    enabled: !root.busy
    implicitHeight: Style.space(36)
    font.family: root.fontFamily
    font.pixelSize: Style.space(12)
    color: root.foreground
    placeholderTextColor: root.dim
    selectionColor: Color.accent
    selectedTextColor: Color.popups.background
    leftPadding: Style.space(10); rightPadding: Style.space(10)
    background: Rectangle {
      radius: Style.space(7)
      color: root.controller.panelFill
      border.width: 1
      border.color: field.activeFocus ? Color.accent : "transparent"
    }
  }
  component Action: SyncthingAction {
    foreground: root.dim
    Keys.onEscapePressed: root.controller.closeAddFolder()
  }

  Dropdown {
    id: pendingFolderPicker
    visible: options.length > 1
    width: parent.width
    showLabel: false
    value: ""
    options: root.controller.pendingFolderOptions()
    foreground: root.foreground
    fontFamily: root.fontFamily
    enabled: !root.busy
    onChanged: function(value) { root.controller.applyPendingFolder(value) }
  }
  Hint {
    readonly property int encryptedCount: root.controller.encryptedPendingOfferCount()
    visible: encryptedCount > 0
    width: parent.width
    text: root.tr(encryptedCount === 1 ? "1 encrypted folder offer requires the Syncthing Web UI."
      : "{count} encrypted folder offers require the Syncthing Web UI.", {count: encryptedCount})
    color: root.warning
  }

  Column {
    width: parent.width
    spacing: Style.space(7)
    Label { text: root.tr("Directory") }
    RowLayout {
      width: parent.width
      spacing: Style.space(8)
      Field {
        id: addPathField
        Layout.fillWidth: true
        placeholderText: "/path/to/existing/folder"
        Accessible.name: root.tr("Existing directory")
      }
      Action {
        iconName: "folder"
        tooltipText: root.tr("Choose an existing directory")
        enabled: !root.busy && !root.folderPickerRunning
        onClicked: root.controller.browseForFolder()
      }
    }
  }
  Column {
    width: parent.width
    spacing: Style.space(7)
    Label { text: root.tr("Label") }
    Field {
      id: addLabelField
      width: parent.width
      placeholderText: root.tr("Directory name if left blank")
      Accessible.name: root.tr("Folder label, optional")
      onTextEdited: root.controller.addLabelFromOffer = false
    }
  }
  Column {
    width: parent.width
    spacing: Style.space(7)
    Label { text: root.tr("Folder ID") }
    RowLayout {
      width: parent.width
      spacing: Style.space(8)
      Field {
        id: addIdField
        Layout.fillWidth: true
        placeholderText: root.syncthing && root.syncthing.folderPreparationBusy ? root.tr("Generating…") : root.tr("Folder identity")
        Accessible.name: root.tr("Folder ID")
        onTextEdited: root.controller.addIdEdited = true
        onAccepted: if (submitButton.enabled) root.controller.submitAddFolder()
      }
      Action {
        iconName: "sync"
        tooltipText: root.tr("Generate a new folder ID")
        enabled: root.syncthing && !root.syncthing.folderPreparationBusy && !root.busy
        onClicked: {
          root.controller.addIdEdited = false
          addIdField.text = ""
          pendingFolderPicker.value = ""
          root.syncthing.requestFolderIdSuggestion()
        }
      }
    }
    Hint {
      width: parent.width
      text: root.tr("Use the same ID to rejoin an existing shared folder.")
    }
  }

  Rectangle { width: parent.width; height: 1; color: root.controller.panelOutline }

  Column {
    width: parent.width
    spacing: Style.space(8)
    RowLayout {
      width: parent.width
      Label { Layout.fillWidth: true; text: root.tr("Share with devices") }
      Hint { text: root.selectedDeviceIds.length ? root.tr("{count} selected", {count: root.selectedDeviceIds.length}) : root.tr("Local only") }
    }
    Flickable {
      id: deviceFlick
      width: parent.width
      height: Math.min(deviceList.implicitHeight, Style.space(170))
      contentHeight: deviceList.implicitHeight
      contentWidth: width
      clip: true
      boundsBehavior: Flickable.StopAtBounds
      interactive: contentHeight > height
      Controls.ScrollBar.vertical: Controls.ScrollBar { policy: Controls.ScrollBar.AsNeeded }
      Column {
        id: deviceList
        width: parent.width
        Repeater {
          model: root.devices
          delegate: Rectangle {
            id: deviceRow
            required property var modelData
            readonly property bool selected: root.selectedDeviceIds.indexOf(String(modelData.value)) >= 0
            width: parent.width
            height: Style.space(34)
            radius: Style.space(7)
            enabled: !root.busy
            color: deviceMouse.containsMouse || activeFocus ? root.controller.panelFill : "transparent"
            border.width: activeFocus ? 1 : 0
            border.color: Color.accent
            activeFocusOnTab: true
            Accessible.role: Accessible.CheckBox
            Accessible.name: modelData.label + ". " + (modelData.description || "")
            Accessible.checked: selected
            Accessible.onToggleAction: root.toggleDevice(String(modelData.value))
            Keys.onReturnPressed: root.toggleDevice(String(modelData.value))
            Keys.onSpacePressed: root.toggleDevice(String(modelData.value))
            onActiveFocusChanged: if (activeFocus) {
              var view = deviceFlick
              view.contentY = Math.max(0, Math.min(view.contentHeight - view.height,
                Math.max(y + height - view.height, Math.min(y, view.contentY))))
            }
            Label {
              anchors.left: parent.left; anchors.leftMargin: Style.space(8)
              anchors.right: deviceCheck.left; anchors.rightMargin: Style.space(8)
              anchors.verticalCenter: parent.verticalCenter
              text: deviceRow.modelData.label
              elide: Text.ElideRight
            }
            SyncthingIcon {
              id: deviceCheck
              anchors.right: parent.right; anchors.rightMargin: Style.space(10)
              anchors.verticalCenter: parent.verticalCenter
              width: Style.space(16); height: width
              name: deviceRow.selected ? "check" : "plus"
              color: deviceRow.selected ? root.success : root.dim
            }
            MouseArea {
              id: deviceMouse
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.toggleDevice(String(deviceRow.modelData.value))
            }
            PanelToolTip {
              visible: deviceMouse.containsMouse
              text: deviceRow.modelData.description || ""
              fontFamily: root.fontFamily
            }
          }
        }
      }
    }
    Hint {
      width: parent.width
      text: root.selectedDeviceIds.length === 0
        ? root.tr("Local only — this folder will not sync with other devices.")
        : root.tr("Selected devices receive a share offer to accept.")
      color: root.selectedDeviceIds.length === 0 ? root.warning : root.dim
    }
  }
  Hint {
    visible: text !== ""
    width: parent.width
    text: root.syncthing ? root.tr(root.syncthing.folderPreparationError) : ""
    color: root.urgent
  }
  RowLayout {
    width: parent.width
    Item { Layout.fillWidth: true }
    Button {
      id: submitButton
      text: root.busy && root.syncthing.folderMutationAction === "add" ? root.tr("Adding…") : root.tr("Add folder")
      fontFamily: root.fontFamily
      fontSize: Style.space(12)
      foreground: root.foreground
      background: Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.12)
      radius: Style.space(7)
      horizontalPadding: Style.space(14)
      verticalPadding: Style.space(8)
      focusable: true
      enabled: root.syncthing && root.syncthing.online && !root.busy
        && String(addPathField.text || "").trim() !== "" && String(addIdField.text || "").trim() !== ""
      opacity: enabled ? 1 : 0.4
      Accessible.role: Accessible.Button
      Accessible.name: text
      Accessible.onPressAction: if (enabled) root.controller.submitAddFolder()
      onClicked: root.controller.submitAddFolder()
    }
  }
}
