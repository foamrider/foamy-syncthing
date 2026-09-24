import QtQuick
import qs.Commons

Row {
  id: root

  property bool active: false
  property string dots: ""
  property string detail: ""
  property string action: ""
  property color foreground: "white"
  property color syncColor: "#26B6DB"
  property color removalColor: "#bf616a"
  property color uploadColor: "#a3be8c"
  property string fontFamily: Style.font.family
  property int fontSize: Style.font.caption

  height: activityLabel.implicitHeight
  spacing: 0

  function actionColor() {
    if (action === "removing") return removalColor
    if (action === "upload") return uploadColor
    return syncColor
  }

  Text {
    textFormat: Text.PlainText
    id: activityLabel
    text: root.active ? "File syncing" : " "
    color: root.syncColor
    font.family: root.fontFamily
    font.pixelSize: root.fontSize
  }

  Item {
    id: dotsSlot
    width: dotsProbe.implicitWidth
    height: dotsProbe.implicitHeight

    Text {
      textFormat: Text.PlainText
      text: root.active ? root.dots : ""
      color: root.syncColor
      font.family: root.fontFamily
      font.pixelSize: root.fontSize
    }

    Text {
      textFormat: Text.PlainText
      id: dotsProbe
      visible: false
      text: "..."
      font.family: root.fontFamily
      font.pixelSize: root.fontSize
    }
  }

  Text {
    textFormat: Text.PlainText
    width: Math.max(0, root.width - activityLabel.implicitWidth
      - dotsSlot.width)
    text: root.active ? " " + root.detail : ""
    color: root.actionColor()
    font.family: root.fontFamily
    font.pixelSize: root.fontSize
    elide: Text.ElideRight
  }
}
