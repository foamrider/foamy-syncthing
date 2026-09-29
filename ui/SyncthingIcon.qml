import QtQuick

Image {
  id: root
  property string name: "folder"
  property color color: "white"
  readonly property var paths: ({
    folder: '<path d="M3 7V5h6l2 2h10v13H3Z"/>',
    sync: '<path d="M20 9a8 8 0 0 0-14-3L3 9m0-5v5h5M4 15a8 8 0 0 0 14 3l3-3m0 5v-5h-5"/>',
    pause: '<rect x="6" y="4" width="4" height="16" rx="1"/><rect x="14" y="4" width="4" height="16" rx="1"/>',
    play: '<path d="m7 4 13 8-13 8Z"/>',
    globe: '<circle cx="12" cy="12" r="9"/><ellipse cx="12" cy="12" rx="4" ry="9"/><path d="M3 12h18"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    unlink: '<path d="m8 3 1 3M3 8l3 1m10 12-1-3m6-2-3-1M10 14l-2 2a4 4 0 0 1-6-6l2-2m10 2 2-2a4 4 0 0 1 6 6l-2 2M3 3l18 18"/>',
    trash: '<path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7"/>',
    download: '<path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5"/>',
    back: '<path d="m11 5-7 7 7 7M4 12h16"/>',
    settings: '<path d="m10 3-.6 2.3-2 .9-2.1-.7-2 3.5 1.6 1.7v2.6L3.3 15l2 3.5 2.1-.7 2 .9L10 21h4l.6-2.3 2-.9 2.1.7 2-3.5-1.6-1.7v-2.6L20.7 9l-2-3.5-2.1.7-2-.9L14 3Z"/><circle cx="12" cy="12" r="3"/>',
    check: '<path d="m5 12 4 4L19 6"/>',
    devices: '<rect x="2" y="3" width="14" height="12" rx="1"/><path d="M9 15v5M5 20h8"/><rect x="17" y="9" width="5" height="12" rx="1"/>',
    chevron: '<path d="m9 5 7 7-7 7"/>',
    external: '<path d="M14 3h7v7m0-7L10 14M10 3H4a1 1 0 0 0-1 1v16a1 1 0 0 0 1 1h16a1 1 0 0 0 1-1v-6"/>',
    disconnected: '<path d="m4 4 16 16M8 3v4m8-4v4M6 7h12v4a6 6 0 0 1-6 6v5M6 11a6 6 0 0 0 2 4"/>',
    alert: '<path d="M12 3 2 21h20Z M12 9v5M12 17h.01"/>'
  })
  sourceSize.width: Math.ceil(width * 2)
  sourceSize.height: Math.ceil(height * 2)
  fillMode: Image.PreserveAspectFit
  // Stroke geometry stays consistent with the other private sans-serif panels.
  source: "data:image/svg+xml;charset=utf-8," + encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="'
    + color.toString() + '" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">'
    + (paths[name] || paths.folder) + '</svg>')
}
