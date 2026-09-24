#!/usr/bin/env bash
# Refuses a release tag that does not name the version this tree builds.
#
# The packages call themselves by the workspace version (`[workspace.package] version` in
# Cargo.toml, which every crate inherits), and every later check — package validation, the
# checksum manifest, the provenance — agrees with the packages. Nothing agreed with the tag, so
# `v0.7.0` could publish a 0.6.2. The release workflow asks this before it builds and again before
# it publishes; a maintainer publishing by hand asks it the same way, first.
#
# usage: scripts/release-version.sh <tag>        e.g. scripts/release-version.sh v0.6.2
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/release-version.sh <tag>" >&2
  exit 2
fi
tag="$1"

version="$(awk '
  /^\[workspace\.package\]/ { section = 1; next }
  /^\[/                     { section = 0 }
  section && /^version[[:space:]]*=/ { gsub(/.*=[[:space:]]*"|".*/, ""); print; exit }
' Cargo.toml)"
if [[ -z "$version" ]]; then
  echo "release-version: Cargo.toml states no [workspace.package] version" >&2
  exit 1
fi

if [[ "$tag" != "v$version" ]]; then
  echo "release-version: the tag is \`$tag\` and this tree builds $version; a release of $version is tagged \`v$version\`" >&2
  exit 1
fi
echo "release-version: $tag names the version this tree builds ($version)"
