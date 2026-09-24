import QtQuick
import "../Translations.js" as Translations

Item {
  id: root
  property string language: "en"
  property int progress: -1
  property color foreground: "white"
  property color accentColor: "white"
  property color trackColor: "gray"
  property color successColor: "green"
  readonly property bool complete: progress >= 100
  readonly property color ringColor: complete ? foreground : accentColor
  implicitWidth: 64
  implicitHeight: 64
  visible: progress >= 0
  Accessible.role: Accessible.Indicator
  Accessible.name: complete ? Translations.text("Up to date", language)
    : Translations.text("{percent}% synchronized", language, {percent: progress})

  Image {
    visible: !root.complete
    anchors.fill: parent
    sourceSize.width: Math.ceil(width * 2)
    sourceSize.height: Math.ceil(height * 2)
    // Re-render only on progress/theme changes; no continuous paint timer.
    source: "data:image/svg+xml;charset=utf-8," + encodeURIComponent(
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" fill="none">'
      + '<circle cx="32" cy="32" r="28" stroke="' + root.trackColor.toString() + '" stroke-width="3"/>'
      + '<circle cx="32" cy="32" r="28" stroke="' + root.ringColor.toString()
      + '" stroke-width="3" stroke-linecap="round" stroke-dasharray="175.929 175.929" stroke-dashoffset="'
      + (175.929 * (1 - Math.max(0, Math.min(100, root.progress)) / 100)).toFixed(3)
      + '" transform="rotate(-90 32 32)"/></svg>')
  }
  Text {
    visible: !root.complete
    anchors.centerIn: parent
    text: root.progress + "%"
    textFormat: Text.PlainText
    font.family: "sans-serif"
    font.pixelSize: root.width * 0.32
    color: root.accentColor
  }
  SyncthingIcon {
    visible: root.complete
    anchors.centerIn: parent
    width: root.width * 0.5; height: width
    name: "check"
    color: root.successColor
  }
}
