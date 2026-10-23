import QtQuick
import Quickshell
import Quickshell.Io
import "plugin/hosts/omarchy/controllers" as Controllers
import "plugin" as Plugin
ShellRoot {
  id: test
  property int stage: 0
  property int index: 0
  property int checks: 0
  readonly property var sections: ["left", "centerBegin", "center", "centerEnd", "tray", "right"]
  Controllers.SettingsController { id: controller }
  FileView { id: fixture; path: controller.settingsPath }
  function check(ok, message) {
    if (!ok) { console.error("FAIL: " + message); Qt.exit(1); return }
    checks++
    console.log("PASS: " + message)
  }
  Timer {
    interval: 300
    running: true
    repeat: true
    onTriggered: {
      switch (test.stage++) {
      case 0:
        test.check(controller.settingsReady, "actual settings controller loads manifest and configuration")
        break
      case 1:
        var layout = {}
        layout[test.sections[test.index]] = [{id:"other.syncthing", language:"en"},
          {id:"foamy.syncthing", language:"nb", probeIntervalSec:45, serviceState:"disabled"}]
        fixture.setText(JSON.stringify({bar:{layout:layout}}))
        break
      case 2:
        test.check(controller.language === Plugin.FoamyI18n.language && controller.probeIntervalSeconds === 45
                   && controller.serviceState === "disabled" && controller.error === "",
                   test.sections[test.index] + " preserves service settings through file watcher")
        if (++test.index < test.sections.length) test.stage = 1
        break
      case 3:
        fixture.setText('{"bar":{"layout":{"tray":["foamy.syncthing"],"centerBegin":null,"centerEnd":{}}}}')
        break
      case 4:
        test.check(controller.probeIntervalSeconds === 15 && controller.serviceState === "enabled",
                   "string and malformed entries use defaults")
        fixture.setText('{broken')
        break
      case 5:
        test.check(controller.error === "Could not read plugin settings", "invalid JSON reports read error")
        fixture.setText('{"bar":{"layout":{"tray":[{"id":"foamy.syncthing","serviceState":"disabled"}]}}}')
        break
      case 6:
        test.check(controller.error === "" && controller.serviceState === "disabled", "valid update recovers after parse error")
        console.log("RESULT: " + test.checks + " settings integration checks passed")
        Qt.quit()
      }
    }
  }
  Timer { interval: 10000; running: true; onTriggered: { console.error("FAIL: timeout"); Qt.exit(1) } }
}
