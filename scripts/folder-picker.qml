import QtQuick
import QtQuick.Dialogs

Window {
  id: root

  readonly property string language: Qt.application.arguments.indexOf("--nb") >= 0 ? "nb" : "en"
  width: 875
  height: 600
  visible: true
  opacity: 0
  flags: Qt.Dialog

  Component.onCompleted: folderDialog.open()

  FolderDialog {
    id: folderDialog
    title: language === "nb" ? "Velg en Syncthing-mappe" : "Choose a Syncthing folder"
    acceptLabel: language === "nb" ? "Velg" : "Choose"
    onAccepted: {
      console.log("SYNCTHING_FOLDER=" + String(selectedFolder))
      Qt.quit()
    }
    onRejected: Qt.quit()
  }
}
