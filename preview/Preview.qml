import QtQuick
import "../kit"
import "../ui" as Ui
import "../service/Facade.js" as Facade
import "../models/PanelModel.js" as PanelModel

// Preview states for the foamy-kit harness (mock data only: no core, no
// processes). The mock projects a core snapshot exactly like Service.qml.
Item {
  id: preview
  required property QtObject bar
  required property Item anchorItem
  property var previewStates: ["folders", "syncing", "devices", "conflicts", "stopped", "not-installed", "empty", "settings", "add-folder", "unlink"]
  property string previewState: "folders"
  property int settleMs: 700
  readonly property Item popup: popup

  readonly property string home: "/home/me"
  readonly property var rawFolders: [
    { id: "docs-x7k2p", label: "Documents", path: home + "/Documents", devices: [{ id: "LAPTOP" }, { id: "PHONE" }],
      status: { state: previewState === "syncing" ? "syncing" : "idle", globalFiles: 1840, globalBytes: 2.4e9,
        needTotalItems: previewState === "syncing" ? 42 : 0, needBytes: previewState === "syncing" ? 6.8e8 : 0 } },
    { id: "photos-q9w3e", label: "Photos", path: home + "/Pictures/Camera", devices: [{ id: "PHONE" }],
      status: { state: "idle", globalFiles: 12873, globalBytes: 38.2e9 } },
    { id: "notes-a1b2c", label: "Notes", path: home + "/Notes", devices: [{ id: "LAPTOP" }],
      status: { state: "idle", globalFiles: 312, globalBytes: 8.1e6 } },
    { id: "work-z8y7x", label: "", path: home + "/Work/Shared", paused: true, devices: [{ id: "LAPTOP" }],
      status: { state: "idle", globalFiles: 96, globalBytes: 4.2e8 } }
  ]

  QtObject {
    id: mock
    readonly property bool stopped: preview.previewState === "stopped"
    readonly property bool missing: preview.previewState === "not-installed"
    readonly property bool empty: preview.previewState === "empty"
    readonly property bool online: !stopped && !missing
    readonly property string localDeviceId: "LOCAL"
    readonly property var devices: Facade.devices(online ? [
      { id: "LOCAL", name: "desktop", connected: true },
      { id: "LAPTOP", name: "ThinkPad", connected: true },
      { id: "PHONE", name: "Pixel 9", connected: preview.previewState !== "devices" }
    ] : [])
    readonly property var folders: Facade.folders(online && !empty ? preview.rawFolders : [])
    readonly property var folderStatuses: Facade.folderStatuses(online && !empty ? preview.rawFolders : [])
    readonly property var conflictFolderIds: preview.previewState === "conflicts" ? ({ "photos-q9w3e": true }) : ({})
    readonly property bool hasConflicts: Object.keys(conflictFolderIds).length > 0
    readonly property var pendingFolders: ({})
    readonly property string syncActivityFolderId: preview.previewState === "syncing" ? "docs-x7k2p" : ""
    readonly property string syncActivityDetail: preview.previewState === "syncing" ? "Report 2026-Q3.pdf" : ""
    readonly property var folderRows: PanelModel.buildFolderRows(mock, preview.home)
    readonly property int folderProblemCount: 0
    readonly property int syncingFolderCount: preview.previewState === "syncing" ? 1 : 0
    readonly property int deviceCount: online ? 2 : 0
    readonly property int connectedDeviceCount: online ? (preview.previewState === "devices" ? 1 : 2) : 0
    readonly property var deviceCounts: PanelModel.deviceCounts(mock)
    readonly property bool tooFewDevices: false
    readonly property bool hasProblems: hasConflicts
    readonly property bool serviceAvailable: !missing
    readonly property bool serviceActive: online
    readonly property string installationState: missing ? "missing" : "existing"
    readonly property bool canInstall: missing
    readonly property bool canControlService: !missing
    readonly property bool serviceActionRunning: false
    readonly property string summaryText: missing ? FoamyI18n.t("Syncthing is not installed") : stopped ? FoamyI18n.t("Syncing stopped") : ""
    readonly property var compactSummary: PanelModel.compactSummary(mock, folderRows, FoamyI18n.t,
      function(key, count, values) { return FoamyI18n.tn(key, count, values) })
    readonly property var pendingOfferRows: []
    readonly property int encryptedPendingOfferCount: 0
    readonly property string recoveryWarning: ""
    readonly property string packageStatus: ""
    readonly property string packageError: ""
    readonly property string conflictScanError: ""
    readonly property string lastError: ""
    readonly property string controlError: ""
    readonly property string folderPreparationError: ""
    readonly property bool folderPreparationBusy: false
    readonly property string folderIdSuggestion: "k3m9q-7xwpd"
    property string folderMutationNotice: ""
    property string folderMutationError: ""
    readonly property bool folderMutationBusy: false
    readonly property string folderMutationAction: ""
    function recover() {}
    function refresh() {}
    function clearFolderMutationMessage() {}
    function requestFolderIdSuggestion() {}
    function addFolder() { return false }
    function setFolderPaused() {}
    function unlinkFolder() {}
    function installSyncthing() {}
    function toggleService() {}
  }

  function apply(view) {
    if (!view) return
    view.forgetFolderId = ""
    view.addOpen = false
    view.tab = previewState === "devices" ? "devices" : "folders"
    var expanded = ({})
    if (previewState === "syncing") expanded["folders:docs-x7k2p"] = true
    if (previewState === "conflicts") expanded["folders:photos-q9w3e"] = true
    if (previewState === "devices") expanded["devices:LAPTOP"] = true
    view.expanded = expanded
    if (previewState === "add-folder") {
      view.openAdd("")
      view.setAddPath(home + "/Music")
    }
    if (previewState === "unlink") view.requestUnlink(PanelModel.folderById(mock.folderRows, "notes-a1b2c"))
  }

  FoamyPopup {
    id: popup
    anchorItem: preview.anchorItem
    bar: preview.bar
    open: true
    body: Component {
      Ui.SyncthingView {
        id: view
        service: mock
        settings: ({ minimumConnectedDevices: 2, refreshIntervalSec: 300, openRefreshIntervalSec: 5 })
        editingSettings: ["settings", "add-folder", "unlink"].indexOf(preview.previewState) >= 0
        Component.onCompleted: Qt.callLater(function() { preview.apply(view) })
        Connections {
          target: preview
          function onPreviewStateChanged() { Qt.callLater(function() { preview.apply(view) }) }
        }
      }
    }
  }
}
