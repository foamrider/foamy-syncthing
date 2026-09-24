#!/bin/bash
set -euo pipefail
widget_id=${1:-}
key=${2:-}
value=${3:-}
[[ -n "$widget_id" && $# == 3 ]] || { echo "Usage: syncthing-preferences.sh WIDGET_ID KEY JSON_VALUE" >&2; exit 2; }
case "$key" in
  language) filter='. == "system" or . == "nb" or . == "en"' ;;
  minimumConnectedDevices) filter='type == "number" and floor == . and . >= 0 and . <= 100' ;;
  refreshIntervalSec) filter='type == "number" and floor == . and . >= 60 and . <= 3600' ;;
  openRefreshIntervalSec) filter='type == "number" and floor == . and . >= 2 and . <= 60' ;;
  probeIntervalSec) filter='type == "number" and floor == . and . >= 5 and . <= 300' ;;
  serviceState) filter='. == "enabled" or . == "disabled"' ;;
  *) echo "Unknown Syncthing preference: $key" >&2; exit 2 ;;
esac
jq -e "$filter" <<<"$value" >/dev/null || { echo "Invalid Syncthing preference: $key" >&2; exit 2; }
# Omarchy owns its live configuration and persists it atomically.
exec omarchy bar set "$widget_id" "$key" "$value" --json
