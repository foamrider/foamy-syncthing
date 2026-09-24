import QtQuick
import QtQuick.Dialogs
import "../Translations.js" as Translations

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
    title: Translations.text("Choose a Syncthing folder", root.language)
    acceptLabel: Translations.text("Choose", root.language)
    onAccepted: {
      console.log("SYNCTHING_FOLDER=" + String(selectedFolder))
      Qt.quit()
    }
    onRejected: Qt.quit()
  }
}
