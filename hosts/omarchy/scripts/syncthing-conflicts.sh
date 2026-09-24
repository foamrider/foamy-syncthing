#!/bin/bash

set -euo pipefail

for folder in "$@"; do
  [[ -d $folder ]] || continue
  conflict_path=$(find -H "$folder" -type f -name '*.sync-conflict-*' -print -quit 2>/dev/null || true)
  if [[ -n $conflict_path ]]; then
    printf 'true\n'
    exit 0
  fi
done

printf 'false\n'
