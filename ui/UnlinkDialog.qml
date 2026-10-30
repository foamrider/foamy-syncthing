import QtQuick
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "../kit"

// A modal confirmation over the popup (unlink a folder). It owns the keys
// while open: Left/Right/Tab pick a button, Enter activates it, Esc cancels.
// The pointer moves the same selection.
FocusScope {
  id: root

  property bool opened: false
  property string message: ""
  property string cancelText: "Cancel"
  property string confirmText: "Confirm"
  // 0 = cancel (the safe default), 1 = confirm.
  property int selectedIndex: 0

  signal canceled()
  signal confirmed()

  visible: opened
  focus: opened
  implicitHeight: card.height + FoamyStyle.padding * 2

  Keys.onPressed: function(event) {
    if (!root.opened) return
    if (event.key === Qt.Key_Escape) root.canceled()
    else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) {
      if (root.selectedIndex === 1) root.confirmed()
      else root.canceled()
    } else if ([Qt.Key_Left, Qt.Key_Right, Qt.Key_Tab, Qt.Key_Backtab, Qt.Key_H, Qt.Key_L].indexOf(event.key) >= 0) {
      root.selectedIndex = root.selectedIndex === 0 ? 1 : 0
    } else return
    event.accepted = true
  }

  Rectangle {
    anchors.fill: parent
    radius: FoamyStyle.radius
    color: Qt.alpha(Commons.Color.popups.background, 0.82)
    MouseArea { anchors.fill: parent; onClicked: root.canceled() }
  }

  Rectangle {
    id: card
    anchors.centerIn: parent
    width: Math.min(parent.width - FoamyStyle.padding * 2, Style.space(360))
    height: column.implicitHeight + FoamyStyle.padding * 2
    radius: FoamyStyle.radius
    color: Commons.Color.popups.background
    border.width: 1
    border.color: FoamyPalette.border
    MouseArea { anchors.fill: parent }

    Column {
      id: column
      anchors.fill: parent
      anchors.margins: FoamyStyle.padding
      spacing: FoamyStyle.gapLarge
      Text {
        width: parent.width
        text: root.message
        textFormat: Text.PlainText
        horizontalAlignment: Text.AlignLeft
        wrapMode: Text.WordWrap
        color: FoamyPalette.text
        font.family: FoamyStyle.fontFamily
        font.pixelSize: FoamyStyle.font.body
      }
      Row {
        anchors.right: parent.right
        spacing: FoamyStyle.gap
        FoamyAction {
          iconName: ""
          label: root.cancelText
          hasCursor: root.selectedIndex === 0
          foreground: FoamyPalette.text
          onHoveredChanged: if (hovered) root.selectedIndex = 0
          onClicked: root.canceled()
        }
        FoamyAction {
          iconName: "unlink"
          label: root.confirmText
          destructive: true
          hasCursor: root.selectedIndex === 1
          foreground: Commons.Color.urgent
          onHoveredChanged: if (hovered) root.selectedIndex = 1
          onClicked: root.confirmed()
        }
      }
    }
  }
}
