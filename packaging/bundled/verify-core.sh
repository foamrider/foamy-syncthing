#!/bin/bash
# Check the bundled native core against packaging/bundled/SHA256SUMS and the
# build metadata pinned in core.pin. Needs coreutils and `file`; the Go build
# metadata check needs `go` (skipped with a note when it is missing).
#
#   packaging/bundled/verify-core.sh
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
pin="$root/packaging/bundled/core.pin"
binary="$root/bin/x86_64/syncshell-core"
value() { sed -n "s/^$1=//p" "$pin"; }
fail() { echo "verify-core: $*" >&2; exit 1; }

[[ -f $binary && ! -L $binary && -x $binary ]] || fail "not a regular executable: $binary"
(cd "$root" && sha256sum --quiet --check packaging/bundled/SHA256SUMS) || fail "checksum mismatch"

metadata=$(file -b -- "$binary")
[[ $metadata == *"ELF 64-bit LSB executable, x86-64"* ]] || fail "not a Linux x86-64 executable: $metadata"
[[ $metadata == *"statically linked"* ]] || fail "not statically linked: $metadata"

if command -v go >/dev/null; then
  info=$(go version -m "$binary")
  expect() { grep -qF -- "$1" <<<"$info" || fail "build metadata lacks: $1"; }
  [[ $(head -n1 <<<"$info") == *": $(value go)" ]] || fail "built with $(head -n1 <<<"$info"), expected $(value go)"
  expect $'path\t'"$(value module)/cmd/syncshell-core"
  expect $'dep\tgolang.org/x/sys\t'"$(value xsys | tr ' ' '\t')"
  expect $'build\t-trimpath=true'
  expect $'build\tCGO_ENABLED=0'
  expect $'build\tGOOS=linux'
  expect $'build\tGOARCH=amd64'
  # No VCS stamp (-buildvcs=false) and no other dependencies.
  ! grep -q $'build\tvcs' <<<"$info" || fail "unexpected VCS metadata"
  [[ $(grep -c $'^\tdep\t' <<<"$info") == 1 ]] || fail "unexpected dependencies"
  echo "verify-core: OK ($(value version), $(value go), $(value commit))"
else
  echo "verify-core: checksum and ELF OK; install go to check the build metadata"
fi
