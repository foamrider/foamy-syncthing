// foamy-kit 0.4.0 bf787e43913cc889af046b3a3ae98ed38ef95372aabac16fe4c44420ad2e8925 — do not edit; change foamy-kit and run sync
import QtQuick
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// The foamy xdg-popup card (menus, hover cards): the host's qs.Ui.PopupCard
// with the foamy radius and border applied from outside, the same way
// FoamyPopup wraps KeyboardPanel. 4.5 places it by bar.barSizeFor/barSize,
// grows it from 0.97 and mirrors it in RTL; 4.0 keeps its own behaviour.
//
// API (everything PopupCard has: anchorItem, bar, owner, open, centerOnBar,
// triggerMode "click"|"hover", containsMouse, fittedContentWidth/Height()), plus:
//   body: Component, bodyItem, bodyImplicitHeight, ensureBody()   lazy content
//   cornerRadius, surfaceColor, cardItem
//   maxHeight                    extra height cap (default 0: the screen's
//                                available height, measured by the host on
//                                4.0 and 4.5 alike; a long menu scrolls)
// Defaults: contentWidth FoamyStyle.menuWidth, padding FoamyStyle.menuPadding,
// height fitted to the body.
PopupCard {
  id: root

  property real cornerRadius: FoamyStyle.radius
  property real maxHeight: 0
  property color surfaceColor: Commons.Color.popups.background

  property Component body: null
  property bool bodyRequested: false
  readonly property Item bodyItem: bodyLoader.item
  readonly property real bodyImplicitHeight: bodyItem ? bodyItem.implicitHeight : 0
  readonly property real menuReserve: bodyLoader.foamyMenuReserve
  readonly property Item cardItem: surfaceHook.card

  function ensureBody() {
    if (body) bodyRequested = true
    return bodyLoader.item
  }

  borderSpec: FoamyStyle.popupBorderSpec
  padding: FoamyStyle.menuPadding
  contentWidth: fittedContentWidth(FoamyStyle.menuWidth)
  // An open FoamyMenu may ask for room below its button (foamyMenuReserve):
  // the card grows while it is open instead of the menu scrolling.
  contentHeight: fittedContentHeight(Math.max(bodyImplicitHeight, bodyLoader.foamyMenuReserve), maxHeight)

  onOpenChanged: if (open && body) bodyRequested = true

  Loader {
    id: bodyLoader
    property real foamyMenuReserve: 0
    anchors.fill: parent
    active: !!root.body && (root.open || root.bodyRequested)
    sourceComponent: root.body
    LayoutMirroring.enabled: FoamyI18n.rightToLeft
    LayoutMirroring.childrenInherit: true
  }

  Item {
    id: surfaceHook
    property Item card: null
    visible: false
    Component.onCompleted: {
      var candidate = parent ? parent.parent : null
      if (candidate && "borderSpec" in candidate && "contentTopInset" in candidate && "radius" in candidate) {
        card = candidate
        candidate.radius = Qt.binding(function() { return root.cornerRadius })
        candidate.color = Qt.binding(function() { return root.surfaceColor })
      } else {
        console.warn("FoamyPopupCard: host card not found; keeping the stock radius")
      }
    }
  }
}
