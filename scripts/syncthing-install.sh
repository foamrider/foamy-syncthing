#!/bin/bash

set -euo pipefail

service_name="syncthing.service"
bin_link="$HOME/.local/bin/syncthing"
service_file="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/$service_name"
script_path="$(readlink -f -- "$0")"

fail() {
  printf 'Error: %s\n' "$*" >&2
  return 1
}

step() {
  printf '\n==> %s\n' "$*"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"
}

# One operation at a time: a run holds an exclusive flock on this file until
# it ends (its terminal closes), and status reports a held lock as
# operationRunning. The kernel drops the lock with the process, so there is no
# marker to remove (or to remove from another run). The file lives only in the
# per-user runtime directory, which no other user can write to; without one
# there is no lock, so nothing runs.
lock_file() {
  # Called in $(…), where errexit is off: return explicitly.
  if [[ ${XDG_RUNTIME_DIR:-} != /* || ! -d $XDG_RUNTIME_DIR ]]; then
    fail "XDG_RUNTIME_DIR is not set to a directory, so the install lock can't be held" || return
  fi
  printf '%s/foamy-syncthing-install.lock\n' "$XDG_RUNTIME_DIR"
}

operation_running() {
  local file="$1" code=0
  [[ -e $file ]] || return 1
  # A shared lock conflicts only with a run's exclusive one.
  flock --nonblock --shared --conflict-exit-code 75 "$file" true || code=$?
  ((code == 75))
}

# The file that keeps a missing Syncthing from being installed: a leftover
# command link or user unit (never replaced or enabled by the install).
blocking_file() {
  local path
  for path in "$bin_link" "$service_file"; do
    if [[ -e $path || -L $path ]]; then
      printf '%s\n' "$path"
      return 0
    fi
  done
}

detect_status() {
  local executable="" executable_path="" blocking="" lock=""
  local flatpak_id="com.github.zocker_160.SyncThingy"
  local label="Not installed" state="missing"

  require_command flock
  lock="$(lock_file)"

  executable="$(command -v syncthing 2>/dev/null || true)"
  if [[ -n $executable ]]; then
    executable_path="$(readlink -f -- "$executable" 2>/dev/null || true)"
    [[ -n $executable_path ]] || executable_path="$executable"
  elif command -v flatpak >/dev/null 2>&1 \
    && timeout 3s flatpak info "$flatpak_id" >/dev/null 2>&1; then
    executable_path="$flatpak_id (Flatpak)"
  fi

  if [[ -n $executable_path ]]; then
    state="existing"
    label="Existing installation found: working"
  else
    blocking="$(blocking_file)"
    if [[ -n $blocking ]]; then
      state="incomplete"
      label="Incomplete installation: non-working"
    fi
  fi

  jq -n \
    --arg state "$state" \
    --arg label "$label" \
    --arg executable "$executable_path" \
    --arg blocking "$blocking" \
    --argjson operationRunning \
    "$(operation_running "$lock" && echo true || echo false)" \
    '{
      state: $state,
      label: $label,
      executable: $executable,
      blocking: $blocking,
      operationRunning: $operationRunning
    }'
}

installation_state() {
  detect_status | jq -r '.state'
}

ensure_initial_install() {
  local state=""
  state="$(installation_state)"
  [[ $state == missing ]] || fail \
    "Syncthing must be absent before installing it (state: $state)"
}

install_package() {
  ensure_initial_install
  require_command omarchy

  step "Installing the official Arch package through Omarchy"
  omarchy pkg add syncthing
  step "Enabling the Syncthing user service"
  systemctl --user daemon-reload
  systemctl --user enable --now "$service_name"
}

run_and_prompt() {
  local action="$1" result=0 lock="" lock_fd=""
  shift

  require_command flock
  lock="$(lock_file)"
  exec {lock_fd}>>"$lock"
  # A status check holds a shared lock for a moment; wait that out.
  flock --exclusive --wait 2 "$lock_fd" \
    || fail "Another Syncthing operation is already running"

  printf 'Syncthing for Omarchy\n'
  printf '%s\n' '----------------------'
  set +e
  (
    set -Eeuo pipefail
    case "$action" in
      install-package) install_package ;;
      *) fail "Unknown operation: $action" ;;
    esac
    # The commands don't get the lock's descriptor, so a daemon one of them
    # leaves behind can't keep the lock after this run ends.
  ) {lock_fd}>&-
  result=$?
  set -e

  if ((result == 0)); then
    printf '\nDone. The Syncthing operation completed successfully.\n'
  else
    printf '\nThe operation failed. Review the error above.\n' >&2
  fi
  printf 'Press Enter to close this terminal. '
  IFS= read -r _
  return "$result"
}

launch_terminal() {
  local action="$1"
  shift
  lock_file >/dev/null
  require_command uwsm-app
  require_command xdg-terminal-exec
  exec uwsm-app -- xdg-terminal-exec --title="Syncthing setup" -- \
    bash "$script_path" run "$action" "$@"
}

main() {
  case "${1:-status}" in
    status) detect_status ;;
    install) launch_terminal install-package ;;
    run)
      shift
      run_and_prompt "$@"
      ;;
    *)
      fail "usage: ${0##*/} {status|install}"
      return 2
      ;;
  esac
}

main "$@"
