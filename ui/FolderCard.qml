import QtQuick
import qs.Commons
import qs.Ui

Rectangle {
  id: root

  property var folder: ({})
  property bool selected: false
  property bool mutationBusy: false
  property string stateLabel: "UNKNOWN"
  property color stateColor: foreground
  property string meta: ""
  property bool activityActive: false
  property string activityDots: ""
  property string activityDetail: ""
  property string activityAction: ""
  property color foreground: Color.foreground
  property color dim: Qt.darker(foreground, 1.5)
  property color urgent: Color.urgent
  property color success: "#a3be8c"
  property color syncColor: "#26B6DB"
  property string fontFamily: Style.font.family

  readonly property bool problem: folder && folder.problem
  readonly property bool syncing: folder && folder.syncing
  readonly property bool canOpen: folder && String(folder.path || "") !== ""
  readonly property color emphasisColor: problem
    ? urgent : (syncing ? syncColor : stateColor)

  signal openRequested
  signal forgetRequested

  implicitHeight: content.implicitHeight + Style.space(16)
  color: rowMouse.containsMouse
    ? Style.hoverFillFor(foreground, Color.accent) : "transparent"
  radius: Style.cornerRadius * 2

  Behavior on color { ColorAnimation { duration: 120 } }

  Rectangle {
    visible: root.selected || root.problem || root.syncing
    anchors.left: parent.left
    anchors.top: parent.top
    anchors.bottom: parent.bottom
    width: Style.space(2)
    radius: width / 2
    color: root.emphasisColor
  }

  MouseArea {
    id: rowMouse
    anchors.fill: parent
    enabled: root.canOpen
    hoverEnabled: true
    cursorShape: root.canOpen ? Qt.PointingHandCursor : Qt.ArrowCursor
    onClicked: root.openRequested()
  }

  Column {
    id: content
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.verticalCenter: parent.verticalCenter
    anchors.leftMargin: Style.space(10)
    anchors.rightMargin: root.folder.paused
      ? forgetButton.width + Style.space(16) : Style.space(10)
    spacing: Style.space(2)

    Item {
      width: parent.width
      implicitHeight: Math.max(nameText.implicitHeight,
        stateText.implicitHeight)

      Text {
        textFormat: Text.PlainText
        id: nameText
        anchors.left: parent.left
        anchors.right: statusDot.left
        anchors.rightMargin: Style.space(8)
        anchors.verticalCenter: parent.verticalCenter
        text: String(root.folder.label || "Unnamed folder")
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        font.bold: true
        elide: Text.ElideRight
      }

      Rectangle {
        id: statusDot
        anchors.right: stateText.left
        anchors.rightMargin: Style.space(5)
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(5)
        height: width
        radius: width / 2
        color: root.stateColor
      }

      Text {
        textFormat: Text.PlainText
        id: stateText
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        text: root.stateLabel.charAt(0)
          + root.stateLabel.slice(1).toLowerCase()
        color: root.stateColor
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        font.bold: true
      }
    }

    ActivityText {
      visible: root.activityActive
      width: parent.width
      active: root.activityActive
      dots: root.activityDots
      detail: root.activityDetail
      action: root.activityAction
      foreground: root.foreground
      syncColor: root.syncColor
      removalColor: root.urgent
      uploadColor: root.success
      fontFamily: root.fontFamily
    }

    Text {
      textFormat: Text.PlainText
      width: parent.width
      text: root.meta
      color: root.problem ? root.urgent : root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      elide: Text.ElideRight
    }

    Text {
      textFormat: Text.PlainText
      visible: root.canOpen
      width: parent.width
      text: String(root.folder.path || "")
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      elide: Text.ElideLeft
    }
  }

  Button {

    radius: Style.cornerRadius * 2
    id: forgetButton
    z: 1
    visible: root.folder && root.folder.paused
    anchors.right: parent.right
    anchors.bottom: parent.bottom
    anchors.margins: Style.space(6)
    width: implicitWidth
    height: implicitHeight
    text: "FORGET"
    tooltipText: "Remove only this unlinked Syncthing configuration"
    bordered: false
    foreground: root.urgent
    fontFamily: root.fontFamily
    fontSize: Style.font.caption
    horizontalPadding: Style.space(5)
    verticalPadding: Style.space(2)
    enabled: !root.mutationBusy
    onClicked: root.forgetRequested()
  }
}
