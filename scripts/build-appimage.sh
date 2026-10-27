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
#
# Works on Fedora and on Ubuntu 22.04. Releases are built on Ubuntu 22.04 by
# .github/workflows/release.yml: an AppImage runs only on systems at least as new as the one it
# was built on (glibc), so building on an old base makes it run on almost every distribution.
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
# Vulkan, capture devices, editing) would only pull in large unrelated libraries. Plugins that
# wrap GPL libraries (x264, a52dec, mpeg2dec, faad) are left out; gst-libav decodes those formats.
wanted="adaptivedemux2 adder alaw alpha alphacolor alsa amrnb amrwbdec apetag app asf
audioconvert audiofx audiomixer audioparsers audiorate audioresample audiotestsrc auparse
autodetect avi cairo compositor coreelements coretracers cutter deinterlace dsd dtmf dvdlpcmdec
dvdsub encoding equalizer flac flv flxdec gdkpixbuf gio icydemux id3demux imagefreeze interleave
isomp4 jpeg lame level libav matroska mpg123 mulaw multifile multipart ogg opengl opus
overlaycomposition pango pbtypes playback png pulseaudio rawparse realmedia replaygain rtp
rtpmanager rtsp shapewipe smpte soup spectrum speex subparse taglib tcp theora twolame
typefindfunctions udp videobox videoconvertscale videocrop videofilter videomixer videorate
videotestsrc volume vorbis vpx wavenc wavpack wavparse xingmux y4m
mse hls dash mpegtsdemux mpegpsdemux videoparsersbad fdkaac openh264 dav1d aom de265 va
pipewire id3tag codecalpha closedcaption ttmlsubs opusparse autoconvert debugutilsbad audiofxbad
videofiltersbad ivfparse subenc proxy soundtouch webp codectimestamper audiobuffersplit dvdspu
dvbsuboverlay videoconvert videoscale"

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

# Anchored at the top: a bare 'dist' would also drop every npm package's own dist/ folder.
rsync -a --delete --exclude '/src-tauri/target' --exclude '/.git' --exclude '/dist' "$project/" "$work/app/"

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

# The embedded player: the LGPL libmpv and FFmpeg from scripts/build-libmpv-lgpl.sh, loaded at
# run time from usr/lib/anix. Anything reporting a GPL or non-free licence stops the build.
libmpv_out="${ANIX_LIBMPV_DIR:-$HOME/.cache/anix-build/libmpv}/out"
[ -f "$libmpv_out/lib/anix/libmpv.so.2" ] || {
  echo "No LGPL libmpv build in $libmpv_out; run scripts/build-libmpv-lgpl.sh first." >&2
  exit 1
}
python3 -I - "$libmpv_out/lib/anix" <<'PY'
import ctypes, glob, os, sys
d = sys.argv[1]
for lib, fn in [("libavutil", "avutil_license"), ("libavcodec", "avcodec_license"), ("libavformat", "avformat_license"),
                ("libavfilter", "avfilter_license"), ("libswscale", "swscale_license"), ("libswresample", "swresample_license")]:
    # Every soname present: the player's FFmpeg and, on older build systems, the one for GStreamer.
    found = [p for p in glob.glob(f"{d}/{lib}.so.*") if os.path.basename(p).count(".") == 2]
    if not found:
        sys.exit(f"{lib} is missing from {d}")
    for path in found:
        f = getattr(ctypes.CDLL(path), fn)
        f.restype = ctypes.c_char_p
        text = f().decode()
        if "LGPL" not in text or "nonfree" in text.lower():
            sys.exit(f"{os.path.basename(path)} reports '{text}', not an LGPL build; refusing.")
mpv = ctypes.CDLL(f"{d}/libmpv.so.2")
mpv.mpv_create.restype = ctypes.c_void_p
mpv.mpv_initialize.argtypes = [ctypes.c_void_p]
mpv.mpv_set_option_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
mpv.mpv_get_property_string.restype = ctypes.c_void_p
mpv.mpv_get_property_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
mpv.mpv_terminate_destroy.argtypes = [ctypes.c_void_p]
h = mpv.mpv_create()
mpv.mpv_set_option_string(h, b"vo", b"null"); mpv.mpv_set_option_string(h, b"ao", b"null")
if mpv.mpv_initialize(h) < 0:
    sys.exit("libmpv does not initialise")
config = ctypes.string_at(mpv.mpv_get_property_string(h, b"mpv-configuration")).decode()
mpv.mpv_terminate_destroy(h)
if "gpl=false" not in config.replace(" ", ""):
    sys.exit("libmpv was not built with -Dgpl=false; refusing.")
print("libmpv and FFmpeg are LGPL builds.")
PY
mkdir -p "$appdir/usr/lib/anix" "$appdir/usr/share/licenses/anix"
cp -P "$libmpv_out"/lib/anix/*.so* "$appdir/usr/lib/anix/"
# Libraries the mpv build links against whose soname differs between distributions (all
# permissively licensed, listed in THIRD-PARTY-NOTICES.md): bundled at the version the build used.
# Audio, X11/Wayland, fonts and VA-API stay the system's.
needed="$(readelf -d "$appdir"/usr/lib/anix/*.so* 2>/dev/null | sed -n 's/.*NEEDED.*\[\(.*\)\].*/\1/p' | sort -u)"
for soname in $(printf '%s\n' "$needed" | grep -E '^lib(dav1d|unwind|lcms2|jpeg)\.so\.[0-9]+$' || true); do
  path="$(ldconfig -p | awk -v n="$soname" '$1 == n && /x86-64/ && !found {print $NF; found = 1}')"
  [ -n "$path" ] || { echo "$soname is not installed on this build machine." >&2; exit 1; }
  cp -L "$path" "$appdir/usr/lib/anix/$soname"
done
cp "$libmpv_out"/share/licenses/* "$appdir/usr/share/licenses/anix/"
cp "$project/THIRD-PARTY-NOTICES.md" "$appdir/usr/share/licenses/anix/"

# The GStreamer libav plugin makes the bundler copy this machine's FFmpeg, which is often a GPL
# build that also drags in GPL codec libraries. Point those sonames at the LGPL build and drop
# what only the GPL build needed.
for lib in "$appdir"/usr/lib/libav*.so* "$appdir"/usr/lib/libsw*.so* "$appdir"/usr/lib/libpostproc.so*; do
  case "$(basename "$lib")" in libavahi*|libavif*) continue ;; esac
  rm -f "$lib"
done
for f in "$appdir"/usr/lib/anix/libav*.so* "$appdir"/usr/lib/anix/libsw*.so*; do
  ln -sf "anix/$(basename "$f")" "$appdir/usr/lib/$(basename "$f")"
done
gpl_libs='x264|x265|xvidcore|vidstab|rubberband|fftw3|chromaprint|leptonica|tesseract|jbig|a52|mpeg2|faad|postproc|dvdread|dvdnav|cdio|cdio_paranoia'
find "$appdir/usr" -regextype posix-extended -regex ".*/lib(${gpl_libs})[._-].*" -print -delete
# libtiff was only there for the GPL build's OCR filter.
if ! find "$appdir/usr" -name '*.so*' -type f -exec readelf -d {} + 2>/dev/null | grep -q 'NEEDED.*\[libtiff\.'; then
  rm -f "$appdir"/usr/lib/libtiff.so*
fi
python3 -I - "$appdir" "$gpl_libs" <<'PY'
import glob, os, re, subprocess, sys
appdir, gpl = sys.argv[1], sys.argv[2]
problems = []
for path in glob.glob(f"{appdir}/usr/**/*.so*", recursive=True):
    name = os.path.basename(path)
    if re.match(rf"lib({gpl})[._-]", name) or re.match(r"libgst(x264|a52dec|mpeg2dec|faad|dvdread|cdparanoia)\.so", name):
        problems.append(f"GPL component bundled: {name}")
    if re.match(r"lib(av(codec|format|filter|util|device)|sw(scale|resample)|postproc)\.so", name):
        target = os.path.realpath(path)
        if os.path.dirname(target) != os.path.realpath(f"{appdir}/usr/lib/anix"):
            problems.append(f"FFmpeg library not from the LGPL build: {name}")
    if os.path.isfile(path) and not os.path.islink(path):
        out = subprocess.run(["readelf", "-d", path], capture_output=True, text=True).stdout
        for needed in re.findall(r"NEEDED.*\[(.+?)\]", out):
            if re.match(rf"lib({gpl})[._-]", needed):
                problems.append(f"{name} still needs the removed {needed}")
if problems:
    sys.exit("\n".join(sorted(set(problems))) + "\nRefusing to build a package with GPL components.")
print("No GPL components in the package.")
PY

# The user name is skipped when it is a generic build account (CI runs as root or runner), whose
# name appears in many unrelated binaries; the home folder is always checked.
check_user="${ANIX_CHECK_USER-$(id -un)}"
if grep -rla -e "$HOME" ${check_user:+-e "$check_user"} "$appdir"; then
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
