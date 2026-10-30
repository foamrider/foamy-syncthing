#!/bin/bash
# Rebuild the bundled native core from its pinned upstream source and compare
# it with bin/x86_64/syncshell-core (packaging/bundled/SHA256SUMS).
#
#   packaging/bundled/build-core.sh            build in a temp dir and compare
#   packaging/bundled/build-core.sh --install  also replace the bundled binary
#
# Uses the upstream build flags (packaging/bundled/build.sh at the pinned
# commit): CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -mod=readonly
# -trimpath -buildvcs=false -ldflags=-buildid=. The output is byte-identical
# only with the pinned Go release (core.pin `go`): GOTOOLCHAIN selects it, so
# the go command may download that toolchain and golang.org/x/sys (checked
# against the pinned go.sum) from the Go proxy. Overrides for offline use:
#   SYNCSHELL_SOURCE=<git dir or URL>  instead of the upstream URL
#   GOTOOLCHAIN=local, GOPROXY=file://…  the usual go environment
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
pin="$root/packaging/bundled/core.pin"
value() { sed -n "s/^$1=//p" "$pin"; }
upstream=$(value upstream)
commit=$(value commit)
toolchain=$(value go)
package=$(value package)
install=false
[[ ${1:-} == --install ]] && install=true

command -v go >/dev/null || { echo "build-core: go is required" >&2; exit 2; }
work=$(mktemp -d "${TMPDIR:-/tmp}/syncshell-core-build.XXXXXX")
trap 'rm -rf -- "$work"' EXIT

git init -q "$work/src"
git -C "$work/src" fetch -q --depth 1 "${SYNCSHELL_SOURCE:-$upstream}" "$commit"
git -C "$work/src" checkout -q FETCH_HEAD
[[ $(git -C "$work/src" rev-parse HEAD) == "$commit" ]] \
  || { echo "build-core: fetched commit is not $commit" >&2; exit 1; }

(
  cd "$work/src/core"
  export GOTOOLCHAIN="${GOTOOLCHAIN:-$toolchain}" GOFLAGS=-mod=readonly \
    GOOS=linux GOARCH=amd64 GOAMD64=v1 CGO_ENABLED=0
  go build -mod=readonly -trimpath -buildvcs=false -ldflags=-buildid= \
    -o "$work/syncshell-core" "$package"
)

built=$(go version "$work/syncshell-core" | awk '{print $2}')
expected=$(awk '{print $1}' "$root/packaging/bundled/SHA256SUMS")
actual=$(sha256sum "$work/syncshell-core" | awk '{print $1}')
echo "built with $built: $actual"
echo "bundled:            $expected"
if [[ $actual == "$expected" ]]; then
  echo "build-core: byte-identical to the bundled binary"
elif [[ $built != "$toolchain" ]]; then
  echo "build-core: differs; expected only because this build used $built, not $toolchain" >&2
  $install || exit 1
else
  echo "build-core: differs from the bundled binary with the pinned toolchain" >&2
  $install || exit 1
fi
if $install; then
  install -m 0755 "$work/syncshell-core" "$root/bin/x86_64/syncshell-core"
  (cd "$root" && sha256sum bin/x86_64/syncshell-core > packaging/bundled/SHA256SUMS)
  echo "build-core: installed; review core.pin and README before committing"
fi
