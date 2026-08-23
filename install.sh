#!/bin/sh
#
# Heretic installer.
#
#   curl -fsSL https://raw.githubusercontent.com/sirsjg/heretic/main/install.sh | sh
#
# Environment:
#   HERETIC_VERSION      release to install (default: latest)
#   HERETIC_APP_DIR      macOS: where Heretic.app goes (default: /Applications)
#   HERETIC_INSTALL_DIR  Linux: where the AppImage goes (default: ~/.local/bin)

set -eu

repository="sirsjg/heretic"

fail() {
  printf 'heretic installer: %s\n' "$*" >&2
  exit 1
}

note() {
  printf '%s\n' "$*"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

require_command awk
require_command curl
require_command mktemp
require_command uname

# --- platform ---------------------------------------------------------------

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=darwin ;;
  *) fail "unsupported operating system: $(uname -s). Heretic runs on macOS and Linux." ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) arch=amd64 ;;
  arm64 | aarch64) arch=arm64 ;;
  *) fail "unsupported architecture: $(uname -m)" ;;
esac

if [ "$os" = linux ] && [ "$arch" != amd64 ]; then
  fail "no prebuilt Linux $arch bundle yet — build from source: https://github.com/$repository#build-from-source"
fi

# --- version ----------------------------------------------------------------

version=${HERETIC_VERSION:-}
if [ -z "$version" ]; then
  latest_url=$(curl -fsSL -o /dev/null -w '%{url_effective}' \
    "https://github.com/$repository/releases/latest") ||
    fail 'could not reach GitHub to resolve the latest release'
  version=${latest_url##*/}
fi
version=${version#v}
case "$version" in
  '' | *[!0-9A-Za-z.-]*) fail "invalid release version: $version" ;;
esac

case "$os" in
  darwin) archive="heretic_${version}_darwin_${arch}.dmg" ;;
  linux) archive="heretic_${version}_linux_${arch}.AppImage" ;;
esac

release_url="https://github.com/$repository/releases/download/v${version}"
temporary_dir=$(mktemp -d 2>/dev/null || mktemp -d -t heretic)

mounted=''
cleanup() {
  [ -z "$mounted" ] || hdiutil detach "$mounted" -quiet >/dev/null 2>&1 || true
  rm -rf "$temporary_dir"
}
trap cleanup 0
trap 'exit 1' HUP INT TERM

# --- download and verify ----------------------------------------------------

note "Downloading Heretic $version for $os/$arch..."
curl -fsSL "$release_url/$archive" -o "$temporary_dir/$archive" ||
  fail "could not download $archive — check that v$version has a $os/$arch build"
curl -fsSL "$release_url/checksums.txt" -o "$temporary_dir/checksums.txt" ||
  fail 'could not download checksums.txt'

expected_checksum=$(
  awk -v archive="$archive" '$2 == archive { print $1; exit }' \
    "$temporary_dir/checksums.txt"
)
[ -n "$expected_checksum" ] || fail "checksum not found for $archive"

if command -v sha256sum >/dev/null 2>&1; then
  actual_checksum=$(sha256sum "$temporary_dir/$archive" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
  actual_checksum=$(shasum -a 256 "$temporary_dir/$archive" | awk '{ print $1 }')
else
  fail 'sha256sum or shasum is required to verify the download'
fi

[ "$actual_checksum" = "$expected_checksum" ] ||
  fail "checksum verification failed for $archive"

# --- install ----------------------------------------------------------------

if [ "$os" = darwin ]; then
  require_command hdiutil

  if [ "${HERETIC_APP_DIR+x}" = x ]; then
    app_dir=$HERETIC_APP_DIR
  else
    app_dir=/Applications
    if [ ! -w "$app_dir" ]; then
      [ -n "${HOME:-}" ] || fail 'HOME is not set; set HERETIC_APP_DIR explicitly'
      app_dir=$HOME/Applications
    fi
  fi
  [ -n "$app_dir" ] || fail 'HERETIC_APP_DIR must not be empty'
  mkdir -p "$app_dir"

  mount_point=$temporary_dir/mnt
  mkdir -p "$mount_point"
  hdiutil attach "$temporary_dir/$archive" -mountpoint "$mount_point" \
    -nobrowse -readonly -quiet || fail 'could not mount the disk image'
  mounted=$mount_point

  source_app=$mount_point/Heretic.app
  [ -d "$source_app" ] || fail 'Heretic.app not found inside the disk image'

  rm -rf "$app_dir/Heretic.app"
  cp -R "$source_app" "$app_dir/Heretic.app" || fail "could not write to $app_dir"

  hdiutil detach "$mounted" -quiet >/dev/null 2>&1 || true
  mounted=''

  # Heretic is ad-hoc signed, not notarised. Downloading with curl does not set
  # the quarantine flag, but clearing it is harmless and covers the case where
  # an earlier copy was installed from a browser download.
  if command -v xattr >/dev/null 2>&1; then
    xattr -dr com.apple.quarantine "$app_dir/Heretic.app" >/dev/null 2>&1 || true
  fi

  note "Installed Heretic $version to $app_dir/Heretic.app"
  note 'Open it with: open -a Heretic'
  exit 0
fi

# Linux
require_command install

if [ "${HERETIC_INSTALL_DIR+x}" = x ]; then
  install_dir=$HERETIC_INSTALL_DIR
else
  [ -n "${HOME:-}" ] || fail 'HOME is not set; set HERETIC_INSTALL_DIR explicitly'
  install_dir=$HOME/.local/bin
fi
[ -n "$install_dir" ] || fail 'HERETIC_INSTALL_DIR must not be empty'

mkdir -p "$install_dir"
install -m 0755 "$temporary_dir/$archive" "$install_dir/heretic" ||
  fail "could not write to $install_dir"

# A desktop entry so it shows up in the launcher rather than only on the PATH.
if [ -n "${HOME:-}" ]; then
  applications_dir=${XDG_DATA_HOME:-$HOME/.local/share}/applications
  icons_dir=${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/128x128/apps
  mkdir -p "$applications_dir" "$icons_dir"

  icon=heretic
  if (cd "$temporary_dir" && "$install_dir/heretic" --appimage-extract heretic.png) \
    >/dev/null 2>&1 && [ -f "$temporary_dir/squashfs-root/heretic.png" ]; then
    cp "$temporary_dir/squashfs-root/heretic.png" "$icons_dir/heretic.png"
  else
    icon=applications-development
  fi

  cat > "$applications_dir/heretic.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Heretic
Comment=Run your Flux board with a team of AI agents
Exec=$install_dir/heretic %U
Icon=$icon
Terminal=false
Categories=Development;
StartupWMClass=Heretic
DESKTOP

  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$applications_dir" >/dev/null 2>&1 || true
  fi
fi

note "Installed Heretic $version to $install_dir/heretic"
case ":${PATH:-}:" in
  *":$install_dir:"*) ;;
  *)
    note "Add it to your PATH with:"
    note "  export PATH=\"$install_dir:\$PATH\""
    ;;
esac
