import QtQuick
import Quickshell
import Quickshell.Io
import "../kit"

QtObject {
  id: root

  required property string helperPath
  property int probeIntervalSeconds: 15
  // Probe periodically only while it matters (Syncthing offline or missing);
  // the owner turns this off while Syncthing is online.
  property bool probing: true
  property string state: "checking"
  property string executablePath: ""
  property bool operationRunning: false
  property bool refreshing: false
  property bool packageActionRunning: false
  property string packageStatus: ""
  property string packageError: ""
  property bool _operationSeen: false
  property bool _statusRefreshPending: false
  property string _statusOutput: ""
  property string _statusErrorOutput: ""

  signal statusApplied

  readonly property bool canInstall: state === "missing"
    && !refreshing && !packageActionRunning

  function updateStatus() {
    if (statusProcess.running) {
      _statusRefreshPending = true
      return
    }
    refreshing = true
    packageError = ""
    _statusOutput = ""
    _statusErrorOutput = ""
    statusProcess.running = true
  }

  function applyStatus(text) {
    var data
    try {
      data = JSON.parse(String(text || ""))
    } catch (error) {
      packageError = FoamyI18n.t("Could not read Syncthing installation status")
      return
    }
    var nextState = String(data.state || "")
    if (["existing", "incomplete", "missing"].indexOf(nextState) < 0) {
      packageError = FoamyI18n.t("Syncthing installation status is invalid")
      return
    }
    state = nextState
    executablePath = String(data.executable || "")
    operationRunning = data.operationRunning === true
    reconcilePackageOperation()
    statusApplied()
  }

  function reconcilePackageOperation() {
    if (packageActionRunning && operationRunning) _operationSeen = true
    if (!packageActionRunning || !_operationSeen || operationRunning) return
    packageActionRunning = false
    packageStatus = FoamyI18n.t("Installation terminal closed")
    _operationSeen = false
    operationPollTimer.stop()
    packageMessageTimer.restart()
  }

  function install() {
    if (!canInstall) return
    packageActionRunning = true
    packageError = ""
    packageStatus = FoamyI18n.t("Complete installation in the Omarchy terminal")
    operationPollTimer.ticks = 0
    _operationSeen = false
    operationPollTimer.start()
    Quickshell.execDetached(["bash", helperPath, "install"])
  }

  property Timer statusTimer: Timer {
    interval: Math.max(1, root.probeIntervalSeconds) * 1000
    repeat: true
    running: root.probing
    triggeredOnStart: true
    onTriggered: root.updateStatus()
  }

  property Timer operationPollTimer: Timer {
    property int ticks: 0
    interval: 1500
    repeat: true
    onTriggered: {
      ticks++
      root.updateStatus()
      if (ticks >= 10 && !root._operationSeen) {
        stop()
        root.packageActionRunning = false
        root.packageStatus = ""
        root.packageError = FoamyI18n.t("The installation terminal did not start")
      }
    }
  }

  property Timer packageMessageTimer: Timer {
    interval: 3000
    repeat: false
    onTriggered: root.packageStatus = ""
  }

  property Process statusProcess: Process {
    command: ["bash", root.helperPath, "status"]

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root._statusOutput = text
    }

    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root._statusErrorOutput = text
    }

    onExited: function(exitCode) {
      var rerun = root._statusRefreshPending
      root.refreshing = false
      if (exitCode === 0) root.applyStatus(root._statusOutput)
      else {
        root.packageError = String(root._statusErrorOutput
          || FoamyI18n.t("Could not check Syncthing installation")).trim()
      }
      root._statusRefreshPending = false
      if (rerun) Qt.callLater(root.updateStatus)
    }
  }
}
