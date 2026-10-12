#!/usr/bin/env bash
# Builds the Anix AppImage on this machine.
#
#   scripts/build-appimage.sh                    uses https://anix-linux.vercel.app
#   ANIX_SITE_URL=https://example.vercel.app scripts/build-appimage.sh
#
# The project is copied to a folder without spaces first (the AppImage tooling breaks on
# them), and the 64-bit GStreamer plugins are handed to the bundler explicitly: on Fedora it
# otherwise picks the 32-bit ones in /usr/lib and the packaged player cannot play anything.
# The finished file is copied to ../release/ next to the project.
set -euo pipefail

project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="${ANIX_BUILD_DIR:-$HOME/.cache/anix-build}"
site="${ANIX_SITE_URL:-https://anix-linux.vercel.app}"

case "$work" in *" "*) echo "ANIX_BUILD_DIR must not contain spaces: $work" >&2; exit 1 ;; esac

plugins_src=""
for dir in /usr/lib64/gstreamer-1.0 "/usr/lib/$(uname -m)-linux-gnu/gstreamer-1.0"; do
  [ -d "$dir" ] && plugins_src="$dir" && break
done
helpers_src=""
for dir in /usr/libexec/gstreamer-1.0 "/usr/lib/$(uname -m)-linux-gnu/gstreamer1.0/gstreamer-1.0"; do
  [ -x "$dir/gst-plugin-scanner" ] && helpers_src="$dir" && break
done
if [ -z "$plugins_src" ] || [ -z "$helpers_src" ]; then
  echo "GStreamer plugins or gst-plugin-scanner not found; install the gstreamer1 plugin packages." >&2
  exit 1
fi

# Plugins the webview needs for MP4 and HLS playback. Everything else on the system (Qt,
# Vulkan, capture devices, editing) would only pull in large unrelated libraries.
wanted="a52dec adaptivedemux2 adder alaw alpha alphacolor alsa amrnb amrwbdec apetag app asf
audioconvert audiofx audiomixer audioparsers audiorate audioresample audiotestsrc auparse
autodetect avi cairo compositor coreelements coretracers cutter deinterlace dsd dtmf dvdlpcmdec
dvdsub encoding equalizer flac flv flxdec gdkpixbuf gio icydemux id3demux imagefreeze interleave
isomp4 jpeg lame level libav matroska mpeg2dec mpg123 mulaw multifile multipart ogg opengl opus
overlaycomposition pango pbtypes playback png pulseaudio rawparse realmedia replaygain rtp
rtpmanager rtsp shapewipe smpte soup spectrum speex subparse taglib tcp theora twolame
typefindfunctions udp videobox videoconvertscale videocrop videofilter videomixer videorate
videotestsrc volume vorbis vpx wavenc wavpack wavparse x264 xingmux y4m
mse hls dash mpegtsdemux mpegpsdemux videoparsersbad fdkaac faad openh264 dav1d aom de265 va
pipewire id3tag codecalpha closedcaption ttmlsubs opusparse autoconvert debugutilsbad audiofxbad
videofiltersbad ivfparse subenc proxy soundtouch webp codectimestamper audiobuffersplit dvdspu
dvbsuboverlay"

rm -rf "$work/gstplugins" "$work/gsthelpers"
mkdir -p "$work/gstplugins" "$work/gsthelpers" "$work/app"
for name in $wanted; do
  [ -f "$plugins_src/libgst$name.so" ] && cp "$plugins_src/libgst$name.so" "$work/gstplugins/"
done
for required in app coreelements playback isomp4 audioparsers videoparsersbad mpegtsdemux libav; do
  [ -f "$work/gstplugins/libgst$required.so" ] || { echo "Missing GStreamer plugin: $required" >&2; exit 1; }
done
cp "$helpers_src/gst-plugin-scanner" "$work/gsthelpers/"
[ -f "$helpers_src/gst-ptp-helper" ] && cp "$helpers_src/gst-ptp-helper" "$work/gsthelpers/"

rsync -a --delete --exclude 'src-tauri/target' --exclude '.git' --exclude 'dist' "$project/" "$work/app/"

cd "$work/app"
echo "Building Anix for $site"
# Keep this machine's folder names (and with them the user name) out of the binary.
export RUSTFLAGS="--remap-path-prefix=$HOME=/build ${RUSTFLAGS:-}"
export CFLAGS="-ffile-prefix-map=$HOME=/build ${CFLAGS:-}"
export CXXFLAGS="-ffile-prefix-map=$HOME=/build ${CXXFLAGS:-}"
ANIX_SITE_URL="$site" NO_STRIP=true CARGO_TARGET_DIR="$work/target" \
  GSTREAMER_PLUGINS_DIR="$work/gstplugins" GSTREAMER_HELPERS_DIR="$work/gsthelpers" \
  npx --no-install tauri build --bundles appimage

bundle="$work/target/release/bundle/appimage"
built="$(ls -t "$bundle"/*.AppImage | head -1)"
appdir="$(ls -d "$bundle"/*.AppDir | head -1)"

# The GTK hook lists every libgiognutls.so it found under /usr/lib*, the second one by its
# path on this machine. Keep the 64-bit module only and point the hook at it inside the
# AppImage, or the packaged webview has no TLS on other machines.
hook="$appdir/apprun-hooks/linuxdeploy-plugin-gtk.sh"
tls_dir=""
while IFS= read -r module; do
  if file -L "$module" | grep -q "ELF 64-bit"; then tls_dir="$(dirname "$module")"; else rm -f "$module"; fi
done < <(find "$appdir/usr" -name libgiognutls.so)
[ -n "$tls_dir" ] || { echo "No 64-bit libgiognutls.so was bundled (install glib-networking)." >&2; exit 1; }
python3 - "$hook" "${tls_dir#"$appdir"/}" <<'PY'
import re, sys
path, relative = sys.argv[1:3]
text = open(path).read()
text, count = re.subn(r'export GIO_EXTRA_MODULES="[^"]*"', 'export GIO_EXTRA_MODULES="$APPDIR/%s"' % relative, text)
if count != 1:
    sys.exit("GIO_EXTRA_MODULES line not found in the GTK hook")
open(path, "w").write(text)
PY

if grep -rla -e "$HOME" -e "$(id -un)" "$appdir"; then
  echo "The package still contains this machine's home folder or user name (files above)." >&2
  exit 1
fi

rm -f "$built"
(cd "$bundle" && ARCH=x86_64 LDAI_OUTPUT="$built" OUTPUT="$built" \
  "$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage" --appdir "$appdir")
[ -f "$built" ] || { echo "Repacking the AppImage failed." >&2; exit 1; }

mkdir -p "$project/../release"
cp "$built" "$project/../release/"
echo "Done: $project/../release/$(basename "$built")"
