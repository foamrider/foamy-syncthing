// foamy-kit 0.15.2 9b8e295ba1d4910ee3699ae9ef3afc0c64eff8ed606a9fa2b4ec7b2ea37e24aa — do not edit; change foamy-kit and run sync
import QtQuick
import qs.Commons as Commons
import qs.Ui
import "FoamyText.js" as FoamyText

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
// Inert and disabled are drawn weaker, not faded: a lighter tint of the
// kind's fill and a label that still reads on it over the popup
// background (inert 4.5:1, disabled 3:1). `foreground` is the label's
// colour in the current state: content a button draws itself takes it,
// since nothing fades with the button.
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

  signal clicked

  readonly property color foreground: _quiet ? _quiet.label : _ink

  // The kind's fill at rest and its label's ink. Destructive: readable on
  // its strongest (pressed) tint, so the label doesn't change on hover.
  readonly property color _restFill: primary ? Commons.Color.accent : destructive ? Qt.alpha(Commons.Color.urgent, 0.12) : FoamyPalette.fieldFill
  readonly property color _ink: primary ? FoamyPalette.accentInk : destructive ? FoamyPalette.onSurface(Commons.Color.urgent, Qt.tint(FoamyPalette.background, Qt.alpha(Commons.Color.urgent, 0.26))) : FoamyPalette.text
  // How strongly a state draws: active and busy 1, inert .7, disabled .45.
  readonly property real _strength: enabled ? (actionEnabled || busy ? 1 : 0.7) : FoamyStyle.disabledOpacity
  // Inert or disabled: { fill (see-through, as drawn), label (opaque) };
  // null when active.
  readonly property var _quiet: _strength < 1 ? _quietLook() : null

  // Fading the whole item moved the fill toward the very colour a primary
  // label has (accentInk is often the background): 2.08:1 inert in the
  // light preview. Each state instead draws the kind's fill with its alpha
  // times the state's mix (inert .7, disabled .45), so it tints the
  // surface under it as the active fill does, and a label measured on that
  // fill over the popup background as drawn (8 bits a channel), with the
  // fill one level nearer the label for the renderer's own rounding of the
  // blend. Where the kind's own ink falls just short at .7 (a dark-ink
  // primary in a dark theme: the dark preview, Tokyo Night), inert's fill
  // steps up, at most to .8, so inert keeps the active button's look. Where
  // it still doesn't read, the fill steps toward the background until an
  // ink does: a primary in a light theme, or with an accent that reads
  // weakly on the background (Nord, Gruvbox: about .3 to .45, with the
  // text's ink). Disabled starts no stronger than inert, so it stays the
  // weaker; a dark-theme primary's disabled label is mostly the text's ink,
  // light where inert's is dark. The label is the faded ink where that
  // reads, else the faintest that does.
  function _quietLook() {
    const inert = 0.7
    const disabled = FoamyStyle.disabledOpacity
    const under = FoamyPalette.background
    // For a primary, the kind's ink is the text or the background: once.
    const inks = [_ink, FoamyPalette.text, FoamyPalette.background].filter((c, i, all) => all.findIndex(o => Qt.colorEqual(o, c)) === i)
    const drawn = mix => Qt.alpha(_restFill, _restFill.a * mix)
    const shown = mix => Qt.color(Qt.tint(under, drawn(mix)).toString())
    const reads = (label, fill, ratio) => {
      const step = (FoamyPalette.luminance(label) > FoamyPalette.luminance(fill) ? 1 : -1) / 255
      const nearer = Qt.rgba(Math.min(1, Math.max(0, fill.r + step)), Math.min(1, Math.max(0, fill.g + step)), Math.min(1, Math.max(0, fill.b + step)), 1)
      return FoamyPalette.contrastRatio(label, nearer) >= ratio
    }
    // The strongest mix from `start` down at which an ink reaches `ratio`;
    // on the background itself, the ink that reads best.
    const pick = (start, ratio) => {
      for (let step = Math.round(start * 100); step > 0; step--) {
        const fill = shown(step / 100)
        const ink = inks.find(c => reads(c, fill, ratio))
        if (ink !== undefined)
          return {
            mix: step / 100,
            fill: fill,
            ink: ink
          }
      }
      const bare = shown(0)
      return {
        mix: 0,
        fill: bare,
        ink: inks.reduce((a, b) => FoamyPalette.contrastRatio(b, bare) > FoamyPalette.contrastRatio(a, bare) ? b : a)
      }
    }
    // The weakest mix from .7 up to .8 at which the kind's own ink reads.
    const rise = () => {
      for (let step = Math.round(inert * 100); step <= 80; step++) {
        const fill = shown(step / 100)
        if (reads(_ink, fill, 4.5))
          return {
            mix: step / 100,
            fill: fill,
            ink: _ink
          }
      }
      return null
    }
    let look = rise() || pick(inert, 4.5)
    if (!enabled)
      look = pick(Math.min(disabled, look.mix * disabled / inert), 3)
    const ratio = enabled ? 4.5 : 3
    let label = look.ink
    for (let step = Math.round(_strength * 100); step <= 100; step++) {
      const faded = Qt.color(Qt.tint(look.fill, Qt.alpha(look.ink, step / 100)).toString())
      if (reads(faded, look.fill, ratio)) {
        label = faded
        break
      }
    }
    return {
      fill: drawn(look.mix),
      label: label
    }
  }

  // The fill at full strength (active or busy): hover and press animate,
  // and so does a change to or from busy under the pointer, since busy
  // draws the rest fill. A change between active, inert and disabled draws
  // at once, so the label never sits on the other state's fill (an
  // animated `color` under the new label read 1.57:1 just after inert to
  // active in the light preview): `color` is bound to the inert or
  // disabled fill, and while the button draws weaker this fill follows the
  // pointer without animating, so it is in place when the button turns
  // active. A theme or `kind` change of an active or busy button still
  // animates its fill under the new label, for one hover time, as before
  // 0.15.2.
  property color _activeFill: {
    if (!busy && mouse.pressed)
      return primary ? Qt.darker(Commons.Color.accent, 1.12) : destructive ? Qt.alpha(Commons.Color.urgent, 0.26) : Qt.alpha(FoamyPalette.text, 0.14)
    if (!busy && (hasCursor || mouse.containsMouse))
      return primary ? Qt.lighter(Commons.Color.accent, 1.1) : destructive ? Qt.alpha(Commons.Color.urgent, 0.2) : Qt.alpha(FoamyPalette.text, 0.1)
    return _restFill
  }
  Behavior on _activeFill {
    enabled: root._strength === 1
    ColorAnimation {
      duration: FoamyMotion.hover
    }
  }
  color: _quiet ? _quiet.fill : _activeFill

  implicitWidth: Math.max(FoamyStyle.controlHeight * 2, content.implicitWidth + FoamyStyle.gapLarge * 2 + border.width * 2)
  implicitHeight: FoamyStyle.controlHeight
  radius: FoamyStyle.radius
  border.width: activeFocus && focusable ? 2 : FoamyStyle.borderWidth
  // Inert and disabled fade the border as the fill; the focus ring stays.
  border.color: {
    if (activeFocus && focusable)
      return primary ? FoamyPalette.text : Commons.Color.accent
    if (primary)
      return "transparent"
    const edge = destructive ? Qt.alpha(Commons.Color.urgent, root.hot ? 0.7 : 0.45) : Qt.alpha(FoamyPalette.text, root.hot ? 0.3 : 0.18)
    return _strength < 1 ? Qt.alpha(edge, edge.a * _strength) : edge
  }
  activeFocusOnTab: focusable && active

  Accessible.role: Accessible.Button
  Accessible.name: text || tooltipText
  Accessible.onPressAction: if (root.active)
    root.clicked()
  Keys.onReturnPressed: if (root.active)
    root.clicked()
  Keys.onEnterPressed: if (root.active)
    root.clicked()
  Keys.onSpacePressed: if (root.active)
    root.clicked()
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
    onClicked: if (root.active)
      root.clicked()
  }

  PanelToolTip {
    id: tip
    visible: mouse.containsMouse && root.tooltipText !== ""
    // The host's tooltip is as wide as its text and wraps only under a cap,
    // so a long label or path would cover the whole popup. Capped through
    // implicitWidth, which the tooltip centers itself by (as the host does).
    implicitWidth: Math.min(implicitContentWidth + leftPadding + rightPadding, Commons.Style.space(320))
    // The caller's text may be app text with no length bound (a path, an
    // error's detail): 3 lines of 200 characters keep an ordinary path whole
    // and the wrapped tooltip a few lines high, with a visible "…" beyond.
    text: FoamyText.cap(root.tooltipText, 3, 200)
    fontFamily: root.fontFamily

    // The host's text wraps at word boundaries only, so a name typed
    // without spaces would run past the capped width.
    Binding {
      target: tip.contentItem
      property: "wrapMode"
      value: Text.Wrap
    }
  }
}
