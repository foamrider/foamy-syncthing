import QtQuick
import QtQuick.Dialogs

// Started by syncthing-folder-picker.sh with the translated title and accept
// label as the last two arguments; prints SYNCTHING_FOLDER=<url> on accept.
Window {
  id: root
  readonly property var labels: Qt.application.arguments.slice(-2)
  title: labels[0]
  width: 875
  height: 600
  visible: true
  opacity: 0
  flags: Qt.Dialog

  Component.onCompleted: folderDialog.open()

  FolderDialog {
    id: folderDialog
    title: root.title
    acceptLabel: root.labels[1]
    onAccepted: {
      console.log("SYNCTHING_FOLDER=" + String(selectedFolder))
      Qt.quit()
    }
    onRejected: Qt.quit()
  }
}
