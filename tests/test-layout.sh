#!/usr/bin/env bash
set -euo pipefail
plugin_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
omarchy_dir=${OMARCHY_PATH:-/usr/share/omarchy}
probe_dir=$(mktemp -d)
trap 'rm -rf -- "$probe_dir"' EXIT
ln -s "$omarchy_dir/shell/Commons" "$probe_dir/Commons"
ln -s "$plugin_dir" "$probe_dir/plugin"
cp "$plugin_dir/tests/test-layout.qml" "$probe_dir/shell.qml"
mkdir -p "$probe_dir/config/omarchy" "$probe_dir/runtime"
chmod 700 "$probe_dir/runtime"
printf '{"bar":{"layout":{}}}\n' > "$probe_dir/config/omarchy/shell.json"
# Only the settings controller runs: no Syncthing core or service actions.
QT_QPA_PLATFORM=offscreen QT_QPA_PLATFORMTHEME=basic XDG_CONFIG_HOME="$probe_dir/config" XDG_RUNTIME_DIR="$probe_dir/runtime" \
  timeout 12s quickshell -p "$probe_dir" --no-color 2>&1 | tee "$probe_dir/result.log"
rg -q 'RESULT: [0-9]+ settings integration checks passed' "$probe_dir/result.log"
if rg 'FAIL:|TypeError:|ReferenceError:|Binding loop|Unable to assign' "$probe_dir/result.log"; then exit 1; fi
