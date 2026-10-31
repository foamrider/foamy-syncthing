pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import qs.Commons as Commons
import qs.Ui
import "../kit"
import "../models/PanelModel.js" as PanelModel

// Add an existing directory: optional incoming share, path (typed or chosen),
// label, folder ID and the devices that receive a share offer. Lives inside a
// FoamySettingsPage (Tab walks the controls; Esc goes back).
Column {
  id: root

  property QtObject service: null
  property bool browsing: false
  property var selectedDeviceIds: []
  property string pendingFolderValue: ""
  property bool idEdited: false
  property bool labelFromOffer: false
  property bool submitted: false

  signal browseRequested()
  signal added()

  readonly property bool busy: !!service && service.folderMutationBusy
  readonly property var devices: service ? PanelModel.deviceOptions(service, FoamyI18n.t) : []
  readonly property var pendingOffers: service ? service.pendingOfferRows : []
  readonly property int encryptedCount: service ? service.encryptedPendingOfferCount : 0
  readonly property string pasteError: pathField.pasteError || labelField.pasteError || idField.pasteError
  readonly property bool canSubmit: !!service && service.online && !busy
    && pathField.text.trim() !== "" && idField.text.trim() !== ""

  spacing: FoamyStyle.sectionGap

  function reset() {
    pathField.text = ""
    labelField.text = ""
    idField.text = ""
    selectedDeviceIds = []
    pendingFolderValue = ""
    idEdited = false
    labelFromOffer = false
    submitted = false
  }
  function focusPath() { pathField.focusInput() }
  function setPath(path) {
    pathField.text = path
    if (labelField.text === "") labelField.text = PanelModel.pathLabel(path)
    Qt.callLater(focusPath)
  }
  function toggleDevice(value) {
    if (busy) return
    // Assign a new list so selection and pending-offer bindings update together.
    var next = selectedDeviceIds.slice()
    var index = next.indexOf(value)
    if (index < 0) next.push(value)
    else next.splice(index, 1)
    selectedDeviceIds = next
  }
  function applyPendingFolder(value) {
    pendingFolderValue = String(value || "")
    selectedDeviceIds = []
    if (!pendingFolderValue) {
      idEdited = false
      idField.text = ""
      if (labelFromOffer) labelField.text = ""
      labelFromOffer = false
      if (service) service.requestFolderIdSuggestion()
      return
    }
    var choice = null
    try { choice = JSON.parse(pendingFolderValue) } catch (error) { return }
    if (!(choice instanceof Array) || choice.length !== 2) return
    var id = String(choice[0] || "")
    var deviceId = String(choice[1] || "")
    var pending = service && service.pendingFolders ? service.pendingFolders[id] || ({}) : ({})
    var offer = (pending.offeredBy || ({}))[deviceId] || ({})
    idEdited = true
    idField.text = id
    labelField.text = String(offer.label || id)
    labelFromOffer = true
    selectedDeviceIds = deviceId ? [deviceId] : []
  }
  function pendingDeviceId() {
    if (!pendingFolderValue) return ""
    try {
      var choice = JSON.parse(pendingFolderValue)
      var deviceId = choice instanceof Array && choice.length === 2 ? String(choice[1] || "") : ""
      return selectedDeviceIds.indexOf(deviceId) >= 0 ? deviceId : ""
    } catch (error) {
      return ""
    }
  }
  function submit() {
    if (!canSubmit) return
    var label = labelField.text.trim() || PanelModel.pathLabel(pathField.text)
    submitted = service.addFolder(pathField.text, label, idField.text.trim(), selectedDeviceIds, pendingDeviceId())
  }

  Connections {
    target: root.service
    function onFolderIdSuggestionChanged() {
      if (!root.idEdited && root.pendingFolderValue === "") idField.text = root.service.folderIdSuggestion
    }
    function onFolderMutationNoticeChanged() {
      if (root.submitted && root.service.folderMutationNotice !== "") {
        root.submitted = false
        root.added()
      }
    }
    function onFolderMutationErrorChanged() {
      if (root.service.folderMutationError !== "") root.submitted = false
    }
  }

  component Label: Text {
    textFormat: Text.PlainText
    horizontalAlignment: Text.AlignLeft
    color: FoamyPalette.text
    font.family: FoamyStyle.fontFamily
    font.pixelSize: FoamyStyle.font.body
  }
  component Hint: Label {
    color: FoamyPalette.mutedText
    font.pixelSize: FoamyStyle.font.bodySmall
    wrapMode: Text.WordWrap
  }
  component Field: SafeTextField {
    implicitHeight: FoamyStyle.controlHeight
    enabled: !root.busy
    translate: FoamyI18n.t
    font.family: FoamyStyle.fontFamily
    font.pixelSize: FoamyStyle.font.body
  }

  FoamySettingsSection {
    visible: root.pendingOffers.length > 0 || root.encryptedCount > 0
    width: parent.width
    title: FoamyI18n.t("Incoming share")
    FoamyDropdown {
      visible: root.pendingOffers.length > 0
      width: parent.width
      showLabel: false
      label: FoamyI18n.t("Incoming share")
      enabled: !root.busy
      value: root.pendingFolderValue
      options: [{ value: "", label: FoamyI18n.t("Create a new folder identity") }].concat(root.pendingOffers.map(function(offer) {
        return { value: offer.value, label: FoamyI18n.t("Accept {name}", { name: offer.label }) }
      }))
      onChanged: function(value) { root.applyPendingFolder(value) }
    }
    Hint {
      visible: root.encryptedCount > 0
      width: parent.width
      text: FoamyI18n.tn("%1 encrypted folder offers require the Syncthing Web UI.", root.encryptedCount)
      color: FoamyPalette.onSurface(FoamyPalette.warning)
    }
  }

  FoamySettingsSection {
    width: parent.width
    title: FoamyI18n.t("Directory")
    RowLayout {
      width: parent.width
      spacing: FoamyStyle.gap
      Field {
        id: pathField
        Layout.fillWidth: true
        Layout.minimumWidth: 0
        placeholderText: "~/Documents"
        Accessible.name: FoamyI18n.t("Existing directory")
      }
      FoamyAction {
        iconName: "folder"
        focusable: true
        tooltipText: FoamyI18n.t("Choose an existing directory")
        actionEnabled: !root.busy && !root.browsing
        foreground: FoamyPalette.secondaryText
        onClicked: root.browseRequested()
      }
    }
    Label { width: parent.width; text: FoamyI18n.t("Label"); color: FoamyPalette.secondaryText; font.pixelSize: FoamyStyle.font.bodySmall }
    Field {
      id: labelField
      width: parent.width
      placeholderText: FoamyI18n.t("Directory name if left blank")
      Accessible.name: FoamyI18n.t("Folder label, optional")
      onTextEdited: root.labelFromOffer = false
    }
    Label { width: parent.width; text: FoamyI18n.t("Folder ID"); color: FoamyPalette.secondaryText; font.pixelSize: FoamyStyle.font.bodySmall }
    RowLayout {
      width: parent.width
      spacing: FoamyStyle.gap
      Field {
        id: idField
        Layout.fillWidth: true
        Layout.minimumWidth: 0
        placeholderText: root.service && root.service.folderPreparationBusy ? FoamyI18n.t("Generating…") : FoamyI18n.t("Folder identity")
        Accessible.name: FoamyI18n.t("Folder ID")
        onTextEdited: root.idEdited = true
        onAccepted: root.submit()
      }
      FoamyAction {
        iconName: "refresh-cw"
        focusable: true
        tooltipText: FoamyI18n.t("Generate a new folder ID")
        actionEnabled: !!root.service && !root.service.folderPreparationBusy && !root.busy
        spinning: !!root.service && root.service.folderPreparationBusy
        foreground: FoamyPalette.secondaryText
        onClicked: {
          root.idEdited = false
          idField.text = ""
          root.pendingFolderValue = ""
          root.service.requestFolderIdSuggestion()
        }
      }
    }
    Hint { width: parent.width; text: FoamyI18n.t("Use the same ID to rejoin an existing shared folder.") }
  }

  FoamySettingsSection {
    width: parent.width
    title: FoamyI18n.t("Share with devices")
    description: root.selectedDeviceIds.length === 0
      ? FoamyI18n.t("Local only — this folder will not sync with other devices.")
      : FoamyI18n.t("Selected devices receive a share offer to accept.")
    Hint {
      visible: root.devices.length === 0
      width: parent.width
      text: FoamyI18n.t("No remote devices configured.")
    }
    Repeater {
      model: root.devices
      delegate: FoamyToggle {
        id: deviceToggle
        required property var modelData
        width: parent ? parent.width : 0
        label: String(modelData.label)
        description: String(modelData.description || "")
        enabled: !root.busy
        checked: root.selectedDeviceIds.indexOf(String(modelData.value)) >= 0
        onClicked: root.toggleDevice(String(deviceToggle.modelData.value))
      }
    }
  }

  Column {
    width: parent.width
    spacing: FoamyStyle.gap
    Hint {
      visible: text !== ""
      width: parent.width
      text: root.pasteError !== "" ? FoamyI18n.t(root.pasteError)
        : root.service ? root.service.folderPreparationError || root.service.folderMutationError : ""
      color: Commons.Color.urgent
      Accessible.role: Accessible.AlertMessage
    }
    FoamyAction {
      anchors.right: parent.right
      iconName: "plus"
      label: root.busy && root.service.folderMutationAction === "add" ? FoamyI18n.t("Adding…") : FoamyI18n.t("Add folder")
      focusable: true
      actionEnabled: root.canSubmit
      foreground: FoamyPalette.text
      onClicked: root.submit()
    }
  }
}
