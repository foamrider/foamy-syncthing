import QtQuick
import qs.Commons as Commons
import "../kit"

// Folder completion ring for the summary: queued bytes against the global
// size, or a check mark when everything is up to date. Re-rendered only when
// the progress or theme changes; nothing ticks.
Item {
  id: root
  property int progress: -1
  property color accentColor: Commons.Color.accent
  property color trackColor: FoamyPalette.fieldFill
  property color successColor: FoamyPalette.onSurface(FoamyPalette.success)
  readonly property bool complete: progress >= 100
  implicitWidth: 64
  implicitHeight: 64
  visible: progress >= 0
  Accessible.role: Accessible.Indicator
  Accessible.name: complete ? FoamyI18n.t("Up to date")
    : FoamyI18n.t("{percent}% synchronized", { percent: FoamyI18n.number(Math.max(0, progress)) })

  Image {
    visible: !root.complete
    anchors.fill: parent
    sourceSize.width: Math.ceil(width * 2)
    sourceSize.height: Math.ceil(height * 2)
    source: "data:image/svg+xml;charset=utf-8," + encodeURIComponent(
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" fill="none">'
      + '<circle cx="32" cy="32" r="28" stroke="' + root.trackColor.toString() + '" stroke-width="3"/>'
      + '<circle cx="32" cy="32" r="28" stroke="' + root.accentColor.toString()
      + '" stroke-width="3" stroke-linecap="round" stroke-dasharray="175.929 175.929" stroke-dashoffset="'
      + (175.929 * (1 - Math.max(0, Math.min(100, root.progress)) / 100)).toFixed(3)
      + '" transform="rotate(-90 32 32)"/></svg>')
  }
  Text {
    visible: !root.complete
    anchors.centerIn: parent
    text: FoamyI18n.t("{percent}%", { percent: FoamyI18n.number(Math.max(0, root.progress)) })
    textFormat: Text.PlainText
    font.family: FoamyStyle.fontFamily
    font.pixelSize: FoamyStyle.font.heading
    color: root.accentColor
  }
  FoamyIcon {
    visible: root.complete
    anchors.centerIn: parent
    width: root.width * 0.5
    height: width
    name: "check"
    color: root.successColor
  }
}
