pragma ComponentBehavior: Bound
// foamy-kit 0.15.1 efd3333a1fe9adb4d5a2ef6c7bec6a6a2e5163bf957022c0d51e56c657732f14 — do not edit; change foamy-kit and run sync
import QtQuick

// A tab row: a full-width row of underline tabs at the top of a popup body,
// directly under the hero band (a view switch such as Inbox / Today /
// Upcoming). Each tab has a label, an optional icon and an optional count;
// the open tab has an accent underline on the row's bottom hairline.
//
//   FoamyTabs {
//     width: parent.width
//     value: root.view
//     model: [
//       { value: "inbox", text: FoamyI18n.t("Inbox"), iconName: "inbox", count: 12 },
//       { value: "today", text: FoamyI18n.t("Today"), iconName: "calendar", count: 3, countUrgent: true },
//       { value: "upcoming", text: FoamyI18n.t("Upcoming"), iconName: "calendar-days" }
//     ]
//     onActivated: function(value) { root.view = value }
//   }
//
// model: plain objects { value, text, iconName?, count?, countUrgent? }.
// A count below 0, or none, shows no count; counts go through
// FoamyI18n.number(). A countUrgent count is drawn in the error colour,
// also on the open tab.
//
// Geometry: height controlHeight + gap; the tabs start `gapLarge` from the
// reading start and pad their content by `rowPadding`, so the first tab's
// icon lines up with the content of FoamyCursorSurface rows below (row
// surface at gapLarge, content at + rowPadding). Each tab is as wide as its
// content. The row draws its own 1px divider along the bottom, so a caller
// only places it. When the tabs don't fit, labels elide from the last tab
// inward; icons and counts keep their size. It never scrolls.
//
// `value` belongs to the caller (DESIGN.md "Controlled values"): a press,
// or Enter/Space on the tab under the cursor, emits activated(value) and
// never assigns `value`. activated() fires for the open tab too; the caller
// decides what that means.
//
// Keyboard: the row is ONE stop of the panel's FoamyCursor. While the panel
// cursor is on it (`hasCursor`), the tab at `cursorIndex` shows the cursor
// highlight; the panel's key catcher passes ←/→ (h/l) to moveCursor(dx) and
// Enter/Space to activateCursor(). moveCursor takes the catcher's dx, which
// is already in reading order (PanelKeyCatcher swaps ←/→ in a mirrored
// panel), so RTL needs nothing more. Arriving by keyboard lands on the open
// tab. The pointer moves the same cursor: a hovered tab takes `cursorIndex`
// and emits hovered(), so the panel moves its cursor here (one highlight on
// screen). Paint comes from the cursor only, never from the pointer directly.
//
//   FoamyCursor { id: cursor; count: rows.length + 1 }        // stop 0: the tabs
//   PanelKeyCatcher {
//     onMoveRequested: function(dx, dy) {
//       if (dy === 0 && cursor.at(0)) {
//         cursor.byKeys = true                                   // the keys' look
//         views.moveCursor(dx)
//       } else
//         cursor.move(dy !== 0 ? dy : dx)
//     }
//     onActivateRequested: cursor.at(0) ? views.activateCursor() : root.activate(cursor.index)
//   }
//   FoamyTabs { id: views; hasCursor: cursor.at(0); cursorByKeys: cursor.byKeys; onHovered: cursor.hover(0); … }
//
// RTL: the tabs follow LayoutMirroring (inherited from the popup), so the
// first tab sits on the reading start.
Item {
  id: root

  property var model: []
  property string value: ""
  property bool hasCursor: false
  // The tab under the panel cursor; -1 while the row doesn't have it.
  property int cursorIndex: -1
  // The keys moved the cursor here (FoamyCursor.byKeys): an accent border.
  property bool cursorByKeys: false
  readonly property int count: Array.isArray(model) ? model.length : 0
  readonly property int selectedIndex: indexOf(value)

  signal activated(string value)
  signal hovered

  // The tabs' leading and trailing inset, and each tab's content padding.
  readonly property real inset: FoamyStyle.gapLarge
  readonly property real tabPadding: FoamyStyle.rowPadding
  readonly property real underlineWidth: FoamyStyle.borderWidth * 2

  // Each label's width: its natural width, less what the row lacks, taken
  // from the last tab first. Reads every tab's natural widths, so it follows
  // text, font weight and scale changes.
  readonly property var labelWidths: fitLabels(tabs.count, tabs.revision, width)

  implicitWidth: FoamyStyle.popupWidth
  implicitHeight: FoamyStyle.controlHeight + FoamyStyle.gap
  opacity: enabled ? 1 : FoamyStyle.disabledOpacity

  Accessible.role: Accessible.PageTabList

  function entry(i) {
    var e = i >= 0 && i < count ? model[i] : null
    return e && typeof e === "object" ? e : ({})
  }
  function indexOf(v) {
    for (let i = 0; i < count; i++) {
      if (String(entry(i).value) === v)
        return i
    }
    return -1
  }
  // revision: itemAt() doesn't notify, so a created or destroyed tab
  // re-evaluates the fit through it.
  function fitLabels(n, revision, available) {
    const natural = []
    let total = inset * 2
    for (let i = 0; i < n; i++) {
      const tab = tabs.itemAt(i) as Tab
      const label = tab ? tab.naturalLabel : 0
      natural.push(label)
      total += (tab ? tab.fixedWidth : 0) + label
    }
    let missing = Math.max(0, total - available)
    for (let i = n - 1; i >= 0 && missing > 0; i--) {
      const cut = Math.min(natural[i], missing)
      natural[i] -= cut
      missing -= cut
    }
    return natural
  }
  // dx in reading order; stops at the ends.
  function moveCursor(delta) {
    if (count <= 0 || delta === 0)
      return
    var from = cursorIndex >= 0 ? cursorIndex : Math.max(0, selectedIndex)
    cursorIndex = Math.max(0, Math.min(count - 1, from + (delta < 0 ? -1 : 1)))
  }
  function activateCursor() {
    if (enabled && cursorIndex >= 0 && cursorIndex < count)
      activated(String(entry(cursorIndex).value))
  }

  // Keyboard arrival lands on the open tab; a hover has already set
  // cursorIndex before the panel hands over the cursor, so it is kept.
  function syncCursor() {
    if (!hasCursor)
      cursorIndex = -1
    else if (cursorIndex < 0)
      // indexOf, not selectedIndex: from onCountChanged that binding may
      // still hold the old model's answer.
      cursorIndex = Math.min(count - 1, Math.max(0, indexOf(value)))
  }
  onHasCursorChanged: syncCursor()
  Component.onCompleted: syncCursor()
  // A shorter model clamps the cursor; a model that gains entries while the
  // row has the cursor (emptied and refilled) lands it on the open tab.
  onCountChanged: {
    if (cursorIndex >= count)
      cursorIndex = count - 1
    syncCursor()
  }

  // One tab: its content, cursor fill and underline. A named type, so
  // fitLabels() reads its natural widths through a checked cast.
  component Tab: Item {
    id: tab
    required property int index
    readonly property var entryData: root.entry(index)
    readonly property bool selected: index === root.selectedIndex
    readonly property bool cursorHere: root.hasCursor && index === root.cursorIndex
    readonly property bool hasIcon: String(entryData.iconName || "") !== ""
    readonly property bool hasCount: typeof entryData.count === "number" && isFinite(entryData.count) && entryData.count >= 0
    readonly property string countText: hasCount ? FoamyI18n.number(Math.floor(entryData.count)) : ""
    // Whole pixels: a fractional width rounds down somewhere and elides.
    readonly property real naturalLabel: Math.ceil(label.implicitWidth)
    // Everything but the label: padding, icon, count and the gaps.
    readonly property real fixedWidth: root.tabPadding * 2 + (hasIcon ? FoamyStyle.iconSize + content.spacing : 0) + (hasCount ? Math.ceil(countLabel.implicitWidth) + content.spacing : 0)
    readonly property real labelWidth: index < root.labelWidths.length ? root.labelWidths[index] : naturalLabel

    objectName: "foamyTab" + index
    width: fixedWidth + labelWidth
    height: parent.height

    Accessible.role: Accessible.PageTab
    Accessible.name: String(entryData.text || "") + (hasCount ? ", " + countText : "")
    Accessible.selectable: true
    Accessible.selected: selected
    Accessible.onPressAction: if (root.enabled)
      root.activated(String(entryData.value))

    // The cursor highlight, inset so it never touches the underline.
    Rectangle {
      objectName: "foamyTabCursor"
      anchors.fill: parent
      anchors.topMargin: FoamyStyle.gapSmall
      anchors.bottomMargin: FoamyStyle.gapSmall
      radius: FoamyStyle.radius
      color: tab.cursorHere ? FoamyPalette.hoverFill : "transparent"
      border.width: tab.cursorHere && root.cursorByKeys ? FoamyStyle.borderWidth : 0
      border.color: FoamyPalette.accent
      Behavior on color {
        ColorAnimation {
          duration: FoamyMotion.hover
        }
      }
    }

    Row {
      id: content
      anchors.left: parent.left
      anchors.leftMargin: root.tabPadding
      anchors.verticalCenter: parent.verticalCenter
      spacing: FoamyStyle.gap

      FoamyIcon {
        objectName: "foamyTabIcon"
        visible: tab.hasIcon
        anchors.verticalCenter: parent.verticalCenter
        width: FoamyStyle.iconSize
        height: FoamyStyle.iconSize
        name: String(tab.entryData.iconName || "")
        color: tab.selected ? FoamyPalette.accent : FoamyPalette.secondaryText
      }
      Text {
        id: label
        objectName: "foamyTabLabel"
        anchors.verticalCenter: parent.verticalCenter
        width: tab.labelWidth
        text: String(tab.entryData.text || "")
        textFormat: Text.PlainText
        horizontalAlignment: Text.AlignLeft
        elide: Text.ElideRight
        maximumLineCount: 1
        color: tab.selected ? FoamyPalette.text : FoamyPalette.secondaryText
        font.family: FoamyStyle.fontFamily
        font.pixelSize: FoamyStyle.font.body
        font.weight: tab.selected ? Font.DemiBold : Font.Normal
      }
      Text {
        id: countLabel
        objectName: "foamyTabCount"
        visible: tab.hasCount
        anchors.verticalCenter: parent.verticalCenter
        text: tab.countText
        textFormat: Text.PlainText
        // bodySmall and secondaryText: a count is read, not decoration. An
        // urgent count reads on the cursor's fill, its strongest surface, so
        // it keeps one colour with and without the cursor.
        color: tab.entryData.countUrgent === true ? FoamyPalette.onSurface(FoamyPalette.error, Qt.tint(FoamyPalette.background, FoamyPalette.hoverFill)) : (tab.selected ? FoamyPalette.accent : FoamyPalette.secondaryText)
        font.family: FoamyStyle.monoFamily
        font.pixelSize: FoamyStyle.font.bodySmall
      }
    }

    // The open tab's underline, on the row's bottom hairline.
    Rectangle {
      objectName: "foamyTabUnderline"
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.bottom: parent.bottom
      height: root.underlineWidth
      color: tab.selected ? FoamyPalette.accent : "transparent"
      Behavior on color {
        ColorAnimation {
          duration: FoamyMotion.hover
        }
      }
    }

    // Pointer: hover moves the panel cursor here, a press activates.
    HoverHandler {
      cursorShape: Qt.PointingHandCursor
      onHoveredChanged: if (hovered) {
        root.cursorIndex = tab.index
        root.hovered()
      }
    }
    TapHandler {
      onTapped: root.activated(String(tab.entryData.value))
    }
  }

  // The hairline under the row; the open tab's underline covers it.
  Rectangle {
    objectName: "foamyTabsHairline"
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.bottom: parent.bottom
    height: FoamyStyle.borderWidth
    color: FoamyPalette.divider
  }

  Row {
    anchors.left: parent.left
    anchors.leftMargin: root.inset
    anchors.top: parent.top
    anchors.bottom: parent.bottom

    Repeater {
      id: tabs
      property int revision: 0
      model: root.count
      onItemAdded: revision++
      onItemRemoved: revision++

      Tab {}
    }
  }
}
