#!/bin/bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
task_tmp=$(mktemp -d)
trap 'rm -rf "$task_tmp"' EXIT
cat > "$task_tmp/omarchy" <<'MOCK'
#!/bin/bash
printf '%s\n' "$@" > "$TEST_PREFERENCES_ARGS"
MOCK
chmod +x "$task_tmp/omarchy"
export PATH="$task_tmp:$PATH" TEST_PREFERENCES_ARGS="$task_tmp/args"
bash "$root/scripts/syncthing-preferences.sh" example.syncthing language '"nb"'
printf '%s\n' bar set example.syncthing language '"nb"' --json > "$task_tmp/expected"
cmp "$task_tmp/args" "$task_tmp/expected"
for pair in 'language "xx"' 'minimumConnectedDevices -1' 'minimumConnectedDevices 2.5' 'openRefreshIntervalSec 1' 'refreshIntervalSec 30' 'unknown true'; do
  read -r key value <<< "$pair"
  rm -f "$task_tmp/args"
  if bash "$root/scripts/syncthing-preferences.sh" example.syncthing "$key" "$value" >/dev/null 2>&1; then
    echo "Accepted invalid preference: $pair" >&2; exit 1
  fi
  [[ ! -e "$task_tmp/args" ]]
done
echo 'Preference persistence arguments and rejection tests passed'
