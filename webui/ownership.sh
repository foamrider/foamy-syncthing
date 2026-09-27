#!/bin/bash

# Bundle revisions describe content; only this marker grants Foamy ownership.
webui_is_owned() {
  local path=$1 marker="$1/.foamy-syncthing-webui"
  [[ -d $path && ! -L $path && -f $marker && ! -L $marker ]] \
    && cmp -s -- "$marker" <(printf '%s\n' 'foamy.syncthing:webui:v1')
}

webui_require_owned_or_absent() {
  local path=$1
  [[ ! -e $path && ! -L $path ]] && return 0
  webui_is_owned "$path" && return 0
  printf 'Refusing to replace unowned Web UI path: %s. Move it aside manually if you want Foamy to install its theme.\n' "$path" >&2
  return 1
}

webui_mark_owned() {
  printf '%s\n' 'foamy.syncthing:webui:v1' >"$1/.foamy-syncthing-webui"
}
