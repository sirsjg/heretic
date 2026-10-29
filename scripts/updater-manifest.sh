#!/usr/bin/env bash
#
# Write the updater manifest (latest.json) for a release's bundles.
#
# The app reads this from the latest release to learn whether there is a newer
# version, where each platform's bundle is, and the signature to check it
# against. build.yml runs it over the collected bundles before uploading them.
#
#   scripts/updater-manifest.sh <version> <bundle-dir> <owner/repo> > latest.json
#
# Environment:
#   NOTES  release notes to show in the app (optional)
#
# Exits 3 without writing anything when the bundles carry no signatures — a
# build without the signing key — so the caller can skip the manifest rather
# than publish one the app would reject.

set -euo pipefail

version=${1:-}
dir=${2:-}
repository=${3:-}

if [ -z "$version" ] || [ -z "$dir" ] || [ -z "$repository" ]; then
  echo "usage: $0 <version> <bundle-dir> <owner/repo>" >&2
  exit 1
fi

command -v jq >/dev/null 2>&1 || { echo "jq is required" >&2; exit 1; }

if ! compgen -G "$dir/*.sig" >/dev/null; then
  echo "no signatures in $dir; skipping the updater manifest" >&2
  exit 3
fi

platforms='{}'

# add <updater target> <bundle file name> <required|optional>
add() {
  local target=$1 file=$2 need=$3
  if [ ! -f "$dir/$file.sig" ]; then
    if [ "$need" = required ]; then
      echo "missing $file.sig: every bundle must be signed, or none" >&2
      exit 1
    fi
    return
  fi
  [ -f "$dir/$file" ] || { echo "missing $file" >&2; exit 1; }

  platforms=$(jq \
    --arg target "$target" \
    --arg url "https://github.com/$repository/releases/download/v$version/$file" \
    --rawfile signature "$dir/$file.sig" \
    '. + {($target): {url: $url, signature: ($signature | gsub("\\s"; ""))}}' \
    <<<"$platforms")
}

add darwin-aarch64 "heretic_${version}_darwin_arm64.app.tar.gz" required
add darwin-x86_64 "heretic_${version}_darwin_amd64.app.tar.gz" required
add linux-x86_64 "heretic_${version}_linux_amd64.AppImage" required
# A .deb install upgrades through dpkg with its own package. Without one the app
# falls back to linking to the release, so it is not an error to lack it.
add linux-x86_64-deb "heretic_${version}_linux_amd64.deb" optional

jq -n \
  --arg version "$version" \
  --arg notes "${NOTES:-}" \
  --arg pub_date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --argjson platforms "$platforms" \
  '{version: $version, notes: $notes, pub_date: $pub_date, platforms: $platforms}'
