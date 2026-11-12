#!/bin/bash
# foamy-syncthing's own tests; tests/validate.sh runs this after the linters.
#   --ci    no Quickshell, no Omarchy host: node, and the helper, the install
#           script and the clipboard helper against fakes (bin/x86_64, and
#           bin/aarch64 too where this machine can run it)
#   --fast  skip the offscreen runtime tests
set -uo pipefail
cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." || exit 1
ci=0 fast=0
for arg in "$@"; do
  case $arg in
    --ci) ci=1 ;;
    --fast) fast=1 ;;
    *)
      echo "suite.sh: unknown argument $arg" >&2
      exit 2
      ;;
  esac
done
status=0
run() {
  echo "== $*"
  "$@" || status=1
}
skip() { echo "SKIP $*"; }
# The QML parts of test_conflicts and test_clipboard start a Quickshell.
((ci)) && export FOAMY_TESTS_NO_QUICKSHELL=1

run node --test tests/*.test.js tests/*.test.cjs
run python3 -I -m unittest discover -s tests -p 'test_*.py'
other=x86_64
[[ $(uname -m) == x86_64 ]] && other=aarch64
if printf '[]' | "bin/$other/foamy-syncthing-helper" conflicts >/dev/null 2>&1; then
  # No core files from the emulator in the plugin tree.
  (
    ulimit -c 0
    export FOAMY_HELPER="[\"bin/$other/foamy-syncthing-helper\"]"
    run python3 -I -m unittest discover -s tests -p 'test_*.py'
    exit "$status"
  ) || status=1
else
  skip "the $other helper: this machine can't run it (qemu binfmt)"
fi
if ((ci || fast)); then
  skip "tests/runtime.py and the kit's i18n_runtime.py: offscreen Quickshell"
else
  # The same default as runtime.py's.
  export FOAMY_KIT="${FOAMY_KIT:-$HOME/GitHub/Personal/foamy-kit}"
  run python3 tests/runtime.py
  run python3 "$FOAMY_KIT/preview/i18n_runtime.py" .
fi
exit $status
