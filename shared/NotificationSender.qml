import QtQuick
import Quickshell.Io

QtObject {
  id: root

  required property string helperPath
  property string error: ""
  readonly property bool busy: _payload !== "" || _queue.length > 0
  property var _queue: []
  property string _payload: ""

  function send(message, visibleMs) {
    if (!message) return false
    var duration = Number(visibleMs)
    var payload = JSON.stringify({summary: "Syncthing", body: String(message),
      expireTimeout: isFinite(duration) && duration > 0
        ? Math.min(2147483647, Math.round(duration)) : -1})
    // Eight bounded requests include the one in flight. Never replace notices.
    // This character limit also keeps UTF-8 below the helper's 32 KiB limit.
    if (payload.length > 8192 || _queue.length + (_payload !== "" ? 1 : 0) >= 8) {
      error = "Could not send desktop notification. Check the result in the panel."
      return false
    }
    _queue.push(payload)
    startNext()
    return true
  }

  function startNext() {
    if (_payload !== "" || !_queue.length) return
    _payload = _queue.shift()
    process.stdinEnabled = true
    deadline.restart()
    process.running = true
  }

  function finish(ok) {
    if (_payload === "") return
    deadline.stop()
    _payload = ""
    error = ok ? "" : "Could not send desktop notification. Check the result in the panel."
    // Wait for the old Process to finish before reusing its stdin channel.
    Qt.callLater(startNext)
  }

  property Process process: Process {
    command: ["/usr/bin/python3", root.helperPath]
    onStarted: {
      write(root._payload + "\n")
      stdinEnabled = false
    }
    onExited: function(code) { root.finish(code === 0) }
  }

  property Timer deadline: Timer {
    interval: 4000
    onTriggered: {
      if (root.process.running) root.process.signal(9)
      else root.finish(false)
    }
  }

  Component.onDestruction: {
    _queue = []
    _payload = ""
    process.running = false
  }
}
