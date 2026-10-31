#!/usr/bin/env bash
# Builds the Anix DEB and RPM packages on this machine.
#
#   scripts/build-packages.sh                    uses https://anix-linux.vercel.app
#   ANIX_SITE_URL=https://example.vercel.app scripts/build-packages.sh
#
# Unlike the AppImage, these packages bundle neither libmpv nor GStreamer: they use the
# system's WebKitGTK and GStreamer (see "depends" in src-tauri/tauri.conf.json) and start with
# the web player. A distribution's own mpv is usually built with GPL parts, which Anix refuses.
# Releases are built on Ubuntu 22.04 by .github/workflows/release.yml, so the packages run on
# Ubuntu 22.04, Debian 12, Fedora 36 and newer. The files are copied to ../release/.
set -euo pipefail

project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="${ANIX_BUILD_DIR:-$HOME/.cache/anix-build}"
site="${ANIX_SITE_URL:-https://anix-linux.vercel.app}"

case "$work" in *" "*) echo "ANIX_BUILD_DIR must not contain spaces: $work" >&2; exit 1 ;; esac
# Absolute, since the build runs from inside it.
mkdir -p "$work" && work="$(cd "$work" && pwd)"
for tool in rsync cpio rpm2cpio dpkg-deb; do
  command -v "$tool" >/dev/null || { echo "$tool is needed to check the packages; install it." >&2; exit 1; }
done

mkdir -p "$work/pkg-app"
# Anchored at the top: a bare 'dist' would also drop every npm package's own dist/ folder.
rsync -a --delete --exclude '/src-tauri/target' --exclude '/.git' --exclude '/dist' "$project/" "$work/pkg-app/"

cd "$work/pkg-app"
echo "Building the Anix DEB and RPM for $site"
# Keep this machine's folder names (and with them the user name) out of the binary.
export RUSTFLAGS="--remap-path-prefix=$HOME=/build ${RUSTFLAGS:-}"
export CFLAGS="-ffile-prefix-map=$HOME=/build ${CFLAGS:-}"
export CXXFLAGS="-ffile-prefix-map=$HOME=/build ${CXXFLAGS:-}"
ANIX_SITE_URL="$site" CARGO_TARGET_DIR="$work/target" npx --no-install tauri build --bundles deb,rpm

bundle="$work/target/release/bundle"
deb="$(ls -t "$bundle"/deb/*.deb | head -1)"
rpm="$(ls -t "$bundle"/rpm/*.rpm | head -1)"

# Same privacy check as the AppImage: unpack both and look for this machine's names.
check="$work/pkg-check"
rm -rf "$check" && mkdir -p "$check/deb" "$check/rpm"
dpkg-deb -x "$deb" "$check/deb"
# Ubuntu 22.04's rpm2cpio (4.17.0) exits 1 even after writing the whole archive, so its status is
# ignored; a short or broken archive still fails in cpio or in the usr/bin/anix check below.
(cd "$check/rpm" && { rpm2cpio "$rpm" || true; } | cpio -idm --quiet)
for root in "$check/deb" "$check/rpm"; do
  [ -x "$root/usr/bin/anix" ] || { echo "No usr/bin/anix in $root" >&2; exit 1; }
  if [ -e "$root/usr/lib/anix/libmpv.so.2" ]; then
    echo "A DEB/RPM must not carry libmpv (only the AppImage has the LGPL build)." >&2
    exit 1
  fi
done
# The user name is skipped when it is a generic build account (CI runs as root or runner), whose
# name appears in many unrelated binaries; the home folder is always checked.
check_user="${ANIX_CHECK_USER-$(id -un)}"
if grep -rla -e "$HOME" ${check_user:+-e "$check_user"} "$check"; then
  echo "A package still contains this machine's home folder or user name (files above)." >&2
  exit 1
fi
rm -rf "$check"

mkdir -p "$project/../release"
cp "$deb" "$rpm" "$project/../release/"
echo "Done: $project/../release/$(basename "$deb") and $(basename "$rpm")"
