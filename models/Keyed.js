.pragma library

// Keep a ListModel of `{ key }` rows in the order of `keys` with the fewest
// inserts, moves and removes. Delegates look their data up by key, so a new
// snapshot with the same folders/devices updates them in place: no delegate
// is rebuilt, and keyboard focus, hover and expansion survive.
function sync(model, keys) {
  var wanted = Object.create(null)
  for (var i = 0; i < keys.length; i++) wanted[keys[i]] = true
  for (var r = model.count - 1; r >= 0; r--) {
    if (!wanted[model.get(r).key]) model.remove(r, 1)
  }
  for (var j = 0; j < keys.length; j++) {
    if (j < model.count && model.get(j).key === keys[j]) continue
    var found = -1
    for (var k = j + 1; k < model.count; k++) {
      if (model.get(k).key === keys[j]) { found = k; break }
    }
    if (found >= 0) model.move(found, j, 1)
    else model.insert(j, { key: keys[j] })
  }
}

// { key: row } for delegate lookups.
function index(rows, keyName) {
  var map = Object.create(null)
  for (var i = 0; i < rows.length; i++) map[String(rows[i][keyName || "id"])] = rows[i]
  return map
}

function keys(rows, keyName) {
  return rows.map(function(row) { return String(row[keyName || "id"]) })
}
