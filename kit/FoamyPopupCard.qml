// foamy-kit 0.2.0 7ec6ca4c2c38c8750c88de29b7bb979251d97a7e7327a154fc8d9be5a38216a7 — do not edit; change foamy-kit and run sync
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
// Defaults: contentWidth FoamyStyle.menuWidth, padding FoamyStyle.menuPadding,
// height fitted to the body.
PopupCard {
  id: root

  property real cornerRadius: FoamyStyle.radius
  property color surfaceColor: Commons.Color.popups.background

  property Component body: null
  property bool bodyRequested: false
  readonly property Item bodyItem: bodyLoader.item
  readonly property real bodyImplicitHeight: bodyItem ? bodyItem.implicitHeight : 0
  readonly property Item cardItem: surfaceHook.card

  function ensureBody() {
    if (body) bodyRequested = true
    return bodyLoader.item
  }

  borderSpec: FoamyStyle.popupBorderSpec
  padding: FoamyStyle.menuPadding
  contentWidth: fittedContentWidth(FoamyStyle.menuWidth)
  contentHeight: fittedContentHeight(bodyImplicitHeight)

  onOpenChanged: if (open && body) bodyRequested = true

  Loader {
    id: bodyLoader
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
