#!/usr/bin/env bash
#
# Write a release version into every manifest that carries one.
#
# semantic-release decides the number from the commit history and calls this as
# its prepare step; the resulting diff is what gets committed and tagged. Run it
# by hand only if you are fixing up a bad release.
#
#   scripts/set-version.sh 0.2.0

set -euo pipefail

version=${1:-}

if [ -z "$version" ]; then
  echo "usage: $0 <version>" >&2
  exit 1
fi

if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
  echo "not a semantic version: $version" >&2
  exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# Each of these carries the version on its own line, at the start of the line,
# so the anchored match cannot stray into a dependency's version field.
perl -pi -e "s/^  \"version\": \".*\"/  \"version\": \"$version\"/ if \$. < 10" package.json
perl -pi -e "s/^  \"version\": \".*\"/  \"version\": \"$version\"/ if \$. < 10" crates/heretic-app/tauri.conf.json
perl -pi -e "s/^version = \".*\"/version = \"$version\"/" Cargo.toml

# Cargo.lock pins the workspace members by version too, and the release build
# runs with --locked, so a stale lockfile would fail the build.
cargo update --workspace --quiet

echo "set version to $version"
