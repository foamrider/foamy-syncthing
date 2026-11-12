// foamy-kit 0.9.0 32aa0fb8fd9f8d3184da86ba58314f4972cbe61c43237a2c560bb05097571e01 — do not edit; change foamy-kit and run sync
import QtQuick

// One bar instance's membership in a FoamyShared scope.
//
//   FoamyMember { id: member; scope: "poller"; demanding: root.opened }
//   Timer {
//     running: member.isLeader
//     interval: member.demanded ? 2000 : 30000
//     onTriggered: Service.refresh()          // state lives in a plugin singleton
//   }
//   FoamyIpc { enabled: member.isLeader; target: "foamy.example"; … }
//
// The leader is the first member still alive; when it goes (its monitor is
// unplugged, the bar reloads) the next one takes over on the next binding pass.
QtObject {
  id: member

  property string scope: "default"
  // This instance wants fresh data now (its popup is open).
  property bool demanding: false

  readonly property bool isLeader: FoamyShared.leader(scope) === member
  readonly property int members: FoamyShared.memberCount(scope)
  readonly property int demand: FoamyShared.demandCount(scope)
  readonly property bool demanded: demand > 0

  property bool _joined: false
  property string _joinedScope: ""

  function _sync() {
    if (!_joined)
      return
    FoamyShared.setDemand(scope, member, demanding)
  }

  onDemandingChanged: _sync()
  onScopeChanged: {
    if (!_joined)
      return
    FoamyShared.leave(_joinedScope, member)
    _joinedScope = scope
    FoamyShared.join(scope, member)
    _sync()
  }
  Component.onCompleted: {
    _joinedScope = scope
    _joined = true
    FoamyShared.join(scope, member)
    _sync()
  }
  Component.onDestruction: FoamyShared.leave(_joinedScope, member)
}
