// foamy-kit 0.4.0 574d30ef6a5837ee493cea7a7195ca0563b456298d7ecb83f683d3380ab18760 — do not edit; change foamy-kit and run sync
import QtQuick
import qs.Commons as Commons
import qs.Ui

// A form button that reads as a button: a filled or bordered surface with a
// label and an optional icon. For actions in forms and settings ("Verify",
// "Apply", "Remove", "Add folder"); icon-only toolbar actions stay FoamyAction.
//
//   FoamyButton { text: FoamyI18n.t("Apply"); kind: "primary"; focusable: true; onClicked: root.apply() }
//   FoamyButton { text: FoamyI18n.t("Verify"); iconName: "check"; busy: root.verifying; onClicked: root.verify() }
//   FoamyButton { text: FoamyI18n.t("Remove"); iconName: "trash"; kind: "destructive"; onClicked: root.remove() }
//
// kind: "secondary" (default; field fill + border), "primary" (accent fill,
// one per form), "destructive" (urgent tint + border).
// Height FoamyStyle.controlHeight, foamy radius, label body size.
//
// Keyboard: like FoamyAction. In a PanelKeyCatcher panel set `hasCursor`
// from the panel's cursor and route `hovered` back; in a form (settings
// page) set `focusable: true` to join the Tab chain (focus ring in the
// accent). Enter/Space activate. `busy` spins the icon (or shows a spinner
// glyph when there is none) and keeps the button inert; actionEnabled: false
// keeps it visible but inert; enabled: false greys it out.
// `tooltipText` only for a short hint; never for paragraphs (DESIGN.md).
Rectangle {
  id: root

  property string text: ""
  property string iconName: ""
  property string kind: "secondary"
  property string tooltipText: ""
  property bool busy: false
  property bool actionEnabled: true
  property bool hasCursor: false
  property bool focusable: false
  property Item keyTarget: null
  property string fontFamily: FoamyStyle.fontFamily
  readonly property bool hovered: mouse.containsMouse
  readonly property bool pressed: mouse.pressed
  readonly property bool active: enabled && actionEnabled && !busy
  readonly property bool primary: kind === "primary"
  readonly property bool destructive: kind === "destructive"
  readonly property bool hot: active && (hasCursor || mouse.containsMouse)

  signal clicked()

  readonly property color foreground: primary ? FoamyPalette.onAccent
    : destructive ? FoamyPalette.onSurface(Commons.Color.urgent) : FoamyPalette.text

  implicitWidth: Math.max(FoamyStyle.controlHeight * 2, content.implicitWidth + FoamyStyle.gapLarge * 2 + border.width * 2)
  implicitHeight: FoamyStyle.controlHeight
  radius: FoamyStyle.radius
  opacity: enabled ? (actionEnabled || busy ? 1 : 0.7) : FoamyStyle.disabledOpacity
  color: {
    if (primary) {
      var base = Commons.Color.accent
      return root.pressed && root.active ? Qt.darker(base, 1.12) : root.hot ? Qt.lighter(base, 1.1) : base
    }
    if (destructive)
      return Qt.alpha(Commons.Color.urgent, root.pressed && root.active ? 0.26 : root.hot ? 0.2 : 0.12)
    return root.pressed && root.active ? Qt.alpha(FoamyPalette.text, 0.14)
      : root.hot ? Qt.alpha(FoamyPalette.text, 0.1) : FoamyPalette.fieldFill
  }
  border.width: activeFocus && focusable ? 2 : FoamyStyle.borderWidth
  border.color: activeFocus && focusable ? (primary ? FoamyPalette.text : Commons.Color.accent)
    : primary ? "transparent"
    : destructive ? Qt.alpha(Commons.Color.urgent, root.hot ? 0.7 : 0.45)
    : Qt.alpha(FoamyPalette.text, root.hot ? 0.3 : 0.18)
  activeFocusOnTab: focusable && active

  Behavior on color { ColorAnimation { duration: FoamyMotion.hover } }

  Accessible.role: Accessible.Button
  Accessible.name: text || tooltipText
  Accessible.onPressAction: if (root.active) root.clicked()
  Keys.onReturnPressed: if (root.active) root.clicked()
  Keys.onEnterPressed: if (root.active) root.clicked()
  Keys.onSpacePressed: if (root.active) root.clicked()
  Keys.forwardTo: keyTarget ? [keyTarget] : []

  Row {
    id: content
    anchors.centerIn: parent
    spacing: FoamyStyle.gap
    FoamyIcon {
      id: icon
      visible: root.iconName !== "" || root.busy
      name: root.busy && root.iconName === "" ? "refresh-cw" : root.iconName
      width: FoamyStyle.iconSize
      height: width
      color: root.foreground
      anchors.verticalCenter: parent.verticalCenter
      RotationAnimation on rotation {
        running: root.busy && root.visible && !FoamyMotion.reduceMotion
        from: 0
        to: 360
        duration: FoamyMotion.spin
        loops: Animation.Infinite
        onStopped: icon.rotation = 0
      }
    }
    Text {
      visible: root.text !== ""
      text: root.text
      textFormat: Text.PlainText
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: FoamyStyle.font.body
      font.weight: root.primary ? Font.DemiBold : Font.Medium
      anchors.verticalCenter: parent.verticalCenter
    }
  }

  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: root.active ? Qt.PointingHandCursor : Qt.ArrowCursor
    onClicked: if (root.active) root.clicked()
  }

  PanelToolTip {
    visible: mouse.containsMouse && root.tooltipText !== ""
    text: root.tooltipText
    fontFamily: root.fontFamily
  }
}
