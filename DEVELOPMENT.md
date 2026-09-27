# Development

Target Omarchy Quattro only. The root Panel.qml, Service.qml, models/, ui/,
Preferences.js, Translations.js and scripts/ implement Foamy's presentation.
The hosts/omarchy/ and shared/ directories contain the Syncshell adapter.
SettingsController.qml is intentionally adapted to read the widget in shell.json,
and adapter add-folder notices use Translations.js. Keep these changes when
updating Syncshell. The native binary and bundled Web UI GUI assets remain
unmodified. Foamy's Web UI installer and theme/removal adapters share
webui/ownership.sh; keep its ownership check in every path that replaces or
deletes a theme directory.

Never store user paths, API keys, Syncthing configuration, or machine preferences
in this repository. Use Omarchy's bar settings API for plugin preferences.
Keep English keys and Norwegian translations in Translations.js; substitute
user names and paths as parameters, never translate them as keys.

Run node --test tests/*.test.js, bash tests/test-preferences.sh,
python3 -m unittest discover -s tests -p 'test_*.py', and
omarchy plugin validate from this checkout. Verify the native binary with
sha256sum -c packaging/bundled/SHA256SUMS. Restart the shell before checking
both languages, overview/settings/add-folder views, narrow layouts, offline
and error states, and keyboard navigation. Do not unlink a user's real folder
to validate removal: tests exercise the native pause/forget contract with mocks.

Syncthing counts remote peers only; the displayed device total includes the
local computer. Native events carry actual sync activity; ordinary refreshes
must not look like synchronization. Do not infer network speeds from folder size.

SyncthingPopup.qml is adapted from Omarchy's Ui/KeyboardPanel.qml. Preserve
its keyboard focus, outside-click dismissal, and multi-monitor behavior.
