pragma Singleton
import QtQml

// Share this plugin's shell-owned controller without asking the replacement
// bar for another plugin's service. The registry never creates a controller.
QtObject {
  property QtObject instance: null
}
