import QtQuick
import QtTest
import Quickshell
import Quickshell.Io
import "../shared" as Shared

Item {
  id: root
  property bool ready: false
  property bool probeFinished: false
  property int probeExit: -1
  Timer { interval: 100; running: true; onTriggered: root.ready = true }
  Shared.NotificationSender {
    id: sender
    helperPath: Quickshell.env("TEST_NOTIFICATION_HELPER")
  }
  Component {
    id: factory
    Shared.NotificationSender { helperPath: Quickshell.env("TEST_NOTIFICATION_HELPER") }
  }
  Process {
    id: probe
    onExited: function(code) { root.probeExit = code; root.probeFinished = true }
  }
  TestCase {
    name: "PrivateNotifications"
    when: root.ready

    function cleanup() {
      tryCompare(sender, "busy", false, 7000)
    }
    function test_1_stdin_and_queue() {
      verify(sender.send("private-Canary-847 📁", 10000))
      verify(sender.send("second", 0))
      tryCompare(sender, "busy", false, 3000)
      compare(sender.error, "")
      compare(sender._queue.length, 0)
    }
    function test_2_failure_preserves_next_notice() {
      verify(sender.send("fail", 0))
      tryVerify(function() { return sender.error !== "" }, 3000)
      verify(sender.send("after failure", 0))
      tryCompare(sender, "busy", false, 3000)
      compare(sender.error, "")
    }
    function test_3_timeout_and_overflow() {
      verify(sender.send("stall", 0))
      for (var i=0; i<7; i++) verify(sender.send("queued " + i, 0))
      verify(!sender.send("overflow", 0))
      verify(sender.error !== "")
      tryCompare(sender, "busy", false, 7000)
      compare(sender._queue.length, 0)
      verify(sender.send("after timeout", 0))
      tryCompare(sender, "busy", false, 3000)
      compare(sender.error, "")
    }
    function test_4_payload_limit_and_missing_helper() {
      verify(!sender.send("x".repeat(8193), 0))
      verify(sender.error !== "")
      var original = sender.helperPath
      sender.helperPath = original + ".missing"
      verify(sender.send("private-Canary-847", 0))
      tryCompare(sender, "busy", false, 5000)
      verify(sender.error !== "")
      sender.helperPath = original
    }
    function test_5_destruction_stops_helper() {
      var worker = factory.createObject(root)
      verify(worker.send("stall", 0))
      tryVerify(function() { return Number(worker.process.processId) > 0 }, 1000)
      var pid = worker.process.processId
      worker.destroy()
      wait(100)
      probe.command = ["/usr/bin/test", "!", "-e", "/proc/" + pid]
      root.probeFinished = false
      probe.running = true
      tryCompare(root, "probeFinished", true, 1000)
      compare(root.probeExit, 0)
    }
    onCompletedChanged: if (completed) {
      console.log("NOTIFICATION_QUEUE passed=" + qtest_results.passCount + " failed=" + qtest_results.failCount)
      Qt.quit()
    }
  }
}
