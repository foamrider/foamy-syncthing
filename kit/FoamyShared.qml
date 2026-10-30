pragma Singleton
// foamy-kit 0.3.3 d32e7f3196ff3fbe26556a521078830a01e99fc523c0ab89dd12a87f5d48d47a — do not edit; change foamy-kit and run sync
import QtQuick

// Once-per-shell coordination for one plugin.
//
// 4.5 draws one bar per monitor, so a bar widget's Panel.qml runs once per
// screen. This singleton lives in the plugin's kit/ directory (registered in
// kit/qmldir), and a qmldir singleton is created once per QML engine: every
// bar instance of this plugin sees the same object, and other plugins have
// their own copy. Instances join a named scope through FoamyMember; the first
// live member is the leader (runs the poller, owns the IPC target), and any
// member can raise demand (its popup is open) so the leader polls faster.
//
// Prefer FoamyMember over calling this directly.
QtObject {
  id: root

  // Bumped on every change so bindings re-evaluate.
  property int revision: 0
  property var scopes: ({})

  function _scope(name) {
    var key = String(name || "default")
    if (!scopes[key]) scopes[key] = { members: [], demand: [] }
    return scopes[key]
  }

  function join(name, member) {
    var scope = _scope(name)
    if (scope.members.indexOf(member) === -1) scope.members.push(member)
    revision++
  }

  function leave(name, member) {
    var scope = _scope(name)
    var index = scope.members.indexOf(member)
    if (index !== -1) scope.members.splice(index, 1)
    var demandIndex = scope.demand.indexOf(member)
    if (demandIndex !== -1) scope.demand.splice(demandIndex, 1)
    revision++
  }

  function setDemand(name, member, wanted) {
    var scope = _scope(name)
    var index = scope.demand.indexOf(member)
    if (wanted && index === -1 && scope.members.indexOf(member) !== -1) scope.demand.push(member)
    else if (!wanted && index !== -1) scope.demand.splice(index, 1)
    else return
    revision++
  }

  function leader(name) {
    revision
    var members = _scope(name).members
    return members.length > 0 ? members[0] : null
  }

  function memberCount(name) {
    revision
    return _scope(name).members.length
  }

  function demandCount(name) {
    revision
    return _scope(name).demand.length
  }
}
