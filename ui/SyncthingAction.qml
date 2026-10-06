import QtQuick
import qs.Commons
import qs.Ui

Rectangle {
  id: root
  property string iconName: "settings"
  property string tooltipText: ""
  property color foreground: Color.foreground
  property color hoverForeground: foreground
  property PanelKeyCatcher keyTarget: null
  signal clicked()
  implicitWidth: Style.space(32)
  implicitHeight: Style.space(32)
  radius: Style.cornerRadius * 2
  opacity: enabled ? 1 : 0.4
  color: mouse.containsMouse || activeFocus
    ? Qt.rgba(foreground.r, foreground.g, foreground.b, 0.08) : "transparent"
  border.width: activeFocus ? 1 : 0
  border.color: Color.accent
  activeFocusOnTab: true
  Accessible.role: Accessible.Button
  Accessible.name: tooltipText
  Accessible.onPressAction: if (enabled) clicked()
  Keys.onReturnPressed: if (enabled) clicked()
  Keys.onEnterPressed: if (enabled) clicked()
  Keys.onSpacePressed: if (enabled) clicked()
  Keys.onEscapePressed: if (keyTarget) keyTarget.closeRequested()
  SyncthingIcon {
    anchors.centerIn: parent
    width: Style.space(16); height: width
    name: root.iconName
    color: mouse.containsMouse ? root.hoverForeground : root.foreground
  }
  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: root.clicked()
  }
  PanelToolTip {
    visible: mouse.containsMouse && root.tooltipText !== ""
    text: root.tooltipText
    fontFamily: "sans-serif"
  }
}
