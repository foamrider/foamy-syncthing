#!/bin/bash

# Record names, types, modes and file contents, including empty directories.
# NUL-delimited records keep filenames with whitespace or newlines unambiguous.
webui_fingerprint() (
  set -euo pipefail
  local inventory unexpected
  inventory=$(mktemp) || exit 1
  trap 'rm -f -- "$inventory"' EXIT
  cd -- "$1" || exit 1
  unexpected=$(find . -mindepth 1 ! -type f ! -type d -print -quit) || exit 1
  [[ -z $unexpected ]] || exit 1
  find . -mindepth 1 ! -path './.foamy-syncthing-webui' \
    -printf '%y %m %p\0' | LC_ALL=C sort -z >"$inventory" || exit 1
  find . -type f ! -path './.foamy-syncthing-webui' -print0 \
    | LC_ALL=C sort -z | xargs -0 -r sha256sum --zero -- >>"$inventory" || exit 1
  sha256sum -- "$inventory" | cut -d ' ' -f 1
)

webui_is_pristine() {
  local path=$1 marker="$1/.foamy-syncthing-webui" fingerprint
  [[ -d $path && ! -L $path && -f $marker && ! -L $marker ]] || return 1
  fingerprint=$(webui_fingerprint "$path") || return 1
  cmp -s -- "$marker" <(printf 'foamy.syncthing:webui:v2\n%s\n' "$fingerprint")
}

webui_require_pristine_or_absent() {
  local path=$1
  [[ ! -e $path && ! -L $path ]] && return 0
  webui_is_pristine "$path" && return 0
  printf 'Refusing to replace unowned or modified Web UI path: %s. Move it aside manually if you want Foamy to install its theme.\n' "$path" >&2
  return 1
}

webui_record_installation() {
  local fingerprint
  fingerprint=$(webui_fingerprint "$1") || return 1
  printf 'foamy.syncthing:webui:v2\n%s\n' "$fingerprint" >"$1/.foamy-syncthing-webui"
}

webui_cleanup_previous() {
  local path=$1
  [[ -e $path || -L $path ]] || return 0
  # Recheck the displaced tree: edits made while staging must survive cleanup.
  if webui_is_pristine "$path"; then
    rm -rf -- "$path"
  else
    printf 'Preserving unowned or modified Web UI path: %s\n' "$path" >&2
  fi
}
