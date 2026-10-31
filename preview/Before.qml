import QtQuick
import qs.Ui
import ".." as Dev
import "../hosts/omarchy/models/FacadeModel.js" as Facade

// The dev (1.0.0) popup for foamy-kit's `render.py --trio` Before column.
// Loaded in a git-archive copy of origin/dev, never by the shell. The dev
// Panel finds its service through ServiceRegistry; a mock with the dev
// service's properties stands in (no native core is started).
Item {
  id: preview
  required property QtObject bar
  required property Item anchorItem
  property var previewStates: ["folders", "syncing", "conflicts", "settings"]
  property string previewState: "folders"
  property int settleMs: 900
  // The dev popup (a KeyboardPanel fork, swapped offscreen by the harness).
  property Item devPopup: null
  // The harness reparents the fork's window item to FakeDisplay.overlay.
  function findPopup() {
    var items = FakeDisplay.overlay ? FakeDisplay.overlay.children : []
    for (var i = 0; i < items.length; i++) if (items[i] && items[i].addPathText !== undefined) return items[i]
    return null
  }
  // The fork's card sits deeper than the harness looks, so hand it over.
  function findCard(item, depth) {
    if (!item || depth > 12) return null
    var kids = item.children || []
    for (var i = 0; i < kids.length; i++)
      if (kids[i] && "borderSpec" in kids[i] && "contentTopInset" in kids[i]) return kids[i]
    for (var j = 0; j < kids.length; j++) {
      var found = findCard(kids[j], depth + 1)
      if (found) return found
    }
    return null
  }
  Item {
    id: popupProxy
    property Item cardItem: null
    readonly property real availableCardHeight: preview.devPopup && preview.devPopup.availableCardHeight !== undefined
      ? preview.devPopup.availableCardHeight : 0
  }
  readonly property Item popup: popupProxy

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
    readonly property bool online: true
    readonly property string phase: "online"
    readonly property bool refreshing: false
    readonly property string localDeviceId: "LOCAL"
    readonly property var devices: Facade.devices([
      { id: "LOCAL", name: "desktop", connected: true },
      { id: "LAPTOP", name: "ThinkPad", connected: true },
      { id: "PHONE", name: "Pixel 9", connected: true }])
    readonly property var folders: Facade.folders(preview.rawFolders)
    readonly property var folderStatuses: Facade.folderStatuses(preview.rawFolders)
    readonly property var pendingFolders: ({})
    readonly property var syncingFiles: []
    readonly property int folderCount: 4
    readonly property int deviceCount: 2
    readonly property int connectedDeviceCount: 2
    readonly property int folderProblemCount: 0
    readonly property int syncingFolderCount: preview.previewState === "syncing" ? 1 : 0
    readonly property bool hasConflicts: preview.previewState === "conflicts"
    readonly property bool serviceAvailable: true
    readonly property bool serviceActive: true
    readonly property string summaryText: "Up to date"
    readonly property string installationState: "existing"
    readonly property bool canInstall: false
    readonly property bool canControlService: true
    readonly property bool serviceActionRunning: false
    readonly property bool folderMutationBusy: false
    readonly property string folderMutationAction: ""
    readonly property string folderMutationError: ""
    readonly property string folderMutationNotice: ""
    readonly property string controlError: ""
    readonly property string folderPreparationError: ""
    readonly property bool folderPreparationBusy: false
    readonly property string folderIdSuggestion: ""
    readonly property string packageError: ""
    readonly property string conflictScanError: ""
    readonly property string lastError: ""
    readonly property string notificationError: ""
    readonly property string recoveryWarning: ""
    readonly property string syncActivityFolderId: preview.previewState === "syncing" ? "docs-x7k2p" : ""
    readonly property string syncActivityDetail: preview.previewState === "syncing" ? "Report 2026-Q3.pdf" : ""
    readonly property string syncActivity: syncActivityDetail ? "File syncing " + syncActivityDetail : ""
    readonly property string syncActivityDots: ""
    readonly property string syncActivityAction: ""
    readonly property string recentlyLinkedFolderId: ""
    readonly property string baseUrl: "http://127.0.0.1:8384"
    function refresh() {}
    function setRefreshInterval() {}
    function clearFolderMutationNotice() {}
    function clearFolderMutationMessage() {}
    function requestFolderIdSuggestion() {}
  }

  Component.onCompleted: Dev.ServiceRegistry.instance = mock

  Dev.Panel {
    id: panel
    parent: preview.anchorItem
    anchors.fill: parent
    bar: preview.bar
    moduleName: "foamy.syncthing"
    settings: ({ minimumConnectedDevices: 2 })
  }

  function apply() {
    if (!panel.opened) panel.open()
    Qt.callLater(function() {
      preview.devPopup = preview.findPopup()
      popupProxy.cardItem = preview.findCard(preview.devPopup, 0)
    })
    if (previewState === "settings") panel.showSettings()
    else panel.hideSettings()
  }
  onPreviewStateChanged: Qt.callLater(apply)
  Timer { interval: 300; running: true; onTriggered: preview.apply() }
}
