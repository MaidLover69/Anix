#!/usr/bin/env bash
# Builds an LGPL libmpv (with an LGPL FFmpeg, libplacebo and libass) for the embedded player.
#
#   scripts/build-libmpv-lgpl.sh                  full build into $ANIX_LIBMPV_DIR/out/lib/anix
#   scripts/build-libmpv-lgpl.sh --configure-only stops after every configure step succeeded
#
# Nothing GPL goes in: FFmpeg is configured without --enable-gpl/--enable-nonfree and with
# --disable-autodetect (only the libraries named below), mpv with -Dgpl=false. The result is
# checked: FFmpeg must report an LGPL licence and mpv's configuration must say gpl=false.
# Everything is installed through the short link /tmp/anix-libmpv-prefix-<uid> (FFmpeg and
# mpv record their prefix, and the real folder would carry the user name) and the libraries
# find each other through RPATH=$ORIGIN. scripts/build-appimage.sh copies out/lib/anix into the AppImage.
#
# Needs: gcc, make, meson (>= 1.3), ninja, pkg-config (pkgconf), patchelf, git, curl, python3
# (with jinja2) and the -devel packages of libva, libdrm, hwdata, wayland, libxkbcommon, libglvnd (EGL),
# pipewire, pulseaudio-libs, alsa-lib, dav1d, freetype, fribidi, harfbuzz, fontconfig and lcms2.
# nasm and wayland-protocols are built here when missing (they are only build tools).
#
# Works on Fedora and on Ubuntu 22.04 (the base of the CI release build, so the AppImage runs on
# older systems too). There mpv's Wayland and PipeWire parts are left out (the system's are too
# old; the AppImage draws through X11 and plays sound through PulseAudio), and a second LGPL
# FFmpeg is built for GStreamer's libav plugin: see "FFmpeg for GStreamer" below.
set -euo pipefail

configure_only=false
[ "${1:-}" = "--configure-only" ] && configure_only=true

root="${ANIX_LIBMPV_DIR:-$HOME/.cache/anix-build/libmpv}"
case "$root" in *" "*) echo "ANIX_LIBMPV_DIR must not contain spaces: $root" >&2; exit 1 ;; esac
jobs="${JOBS:-$(nproc)}"
src="$root/src"
tools="$root/tools"
out="$root/out"
mkdir -p "$src" "$root/prefix" "$tools/bin"
prefix="/tmp/anix-libmpv-prefix-$(id -u)"
ln -sfn "$root/prefix" "$prefix"

NASM_VERSION=2.16.03
WAYLAND_PROTOCOLS_TAG=1.47
LIBDISPLAY_INFO_TAG=0.2.0
LIBASS_VERSION=0.17.4
LIBPLACEBO_TAG=v7.351.0
FFMPEG_VERSION=8.0.1
# FFmpeg releases matching each libavcodec soname a system's gst-libav may be linked against.
gst_ffmpeg_for() { case "$1" in 58) echo 4.4.6 ;; 59) echo 5.1.6 ;; 60) echo 6.1.2 ;; 61) echo 7.1.1 ;; esac; }
MPV_TAG=v0.41.0

export PATH="$tools/bin:$PATH"
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig:$prefix/share/pkgconfig:$tools/share/pkgconfig"
# Keep build paths out of the binaries (they would carry the user name).
export CFLAGS="-O2 -ffile-prefix-map=$root=/build ${CFLAGS:-}"
export CXXFLAGS="$CFLAGS"

fetch() { # url file
  [ -f "$src/$2" ] || { curl -fL --retry 5 --retry-all-errors -o "$src/$2.part" "$1" && mv "$src/$2.part" "$src/$2"; }
}
unpack() { # file dir
  [ -d "$src/$2" ] || { mkdir -p "$src/$2" && tar -xf "$src/$1" -C "$src/$2" --strip-components=1; }
}
clone() { # url tag dir
  [ -d "$src/$3" ] || git clone --depth 1 --recurse-submodules --shallow-submodules --branch "$2" "$1" "$src/$3"
}
step() { echo; echo "==> $*"; }
t0=$(date +%s)

# --- build tools --------------------------------------------------------------------------
if ! command -v nasm >/dev/null; then
  step "nasm $NASM_VERSION (build tool)"
  fetch "https://www.nasm.us/pub/nasm/releasebuilds/$NASM_VERSION/nasm-$NASM_VERSION.tar.xz" nasm.tar.xz
  unpack nasm.tar.xz nasm
  (cd "$src/nasm" && ./configure --prefix="$tools" >/dev/null && make -j"$jobs" >/dev/null && make install >/dev/null)
fi
if ! pkg-config --exists wayland-protocols; then
  step "wayland-protocols $WAYLAND_PROTOCOLS_TAG (build data)"
  clone https://gitlab.freedesktop.org/wayland/wayland-protocols.git "$WAYLAND_PROTOCOLS_TAG" wayland-protocols
  meson setup --reconfigure "$src/wayland-protocols/build" "$src/wayland-protocols" --prefix="$tools" -Dtests=false >/dev/null
  meson install -C "$src/wayland-protocols/build" >/dev/null
fi

meson_lib() { # dir, options...
  local dir="$1"; shift
  meson setup --reconfigure "$dir/build" "$dir" --prefix="$prefix" --libdir=lib --buildtype=release \
    -Ddefault_library=shared "$@"
  $configure_only && return 0
  ninja -C "$dir/build" -j"$jobs"
  meson install -C "$dir/build"
}

# --- libdisplay-info (MIT), only where the system lacks it ----------------------------------
# mpv's DRM support (and with it VA-API hardware decoding) needs it; Ubuntu 22.04 has none.
# Built here, it is bundled next to libmpv.
if ! pkg-config --exists 'libdisplay-info >= 0.1.1'; then
  step "libdisplay-info $LIBDISPLAY_INFO_TAG"
  clone https://gitlab.freedesktop.org/emersion/libdisplay-info.git "$LIBDISPLAY_INFO_TAG" libdisplay-info
  meson_lib "$src/libdisplay-info"
fi

# --- libass (ISC) -------------------------------------------------------------------------
step "libass $LIBASS_VERSION"
fetch "https://github.com/libass/libass/releases/download/$LIBASS_VERSION/libass-$LIBASS_VERSION.tar.xz" libass.tar.xz
unpack libass.tar.xz libass
(cd "$src/libass" && ./configure --prefix="$prefix" --disable-static --enable-shared)
if ! $configure_only; then make -C "$src/libass" -j"$jobs" && make -C "$src/libass" install; fi

# --- libplacebo (LGPL-2.1+) ---------------------------------------------------------------
step "libplacebo $LIBPLACEBO_TAG"
clone https://code.videolan.org/videolan/libplacebo.git "$LIBPLACEBO_TAG" libplacebo
meson_lib "$src/libplacebo" -Dvulkan=disabled -Dshaderc=disabled -Dglslang=disabled -Dd3d11=disabled \
  -Dopengl=enabled -Dlcms=enabled -Dlibdovi=disabled -Dxxhash=disabled -Ddemos=false -Dtests=false

# --- FFmpeg (LGPL-2.1+) -------------------------------------------------------------------
step "FFmpeg $FFMPEG_VERSION"
fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" ffmpeg.tar.xz
unpack ffmpeg.tar.xz ffmpeg
# Network protocols stay (mpv reads from Anix's loopback proxy); TLS is not needed for that.
(cd "$src/ffmpeg" && ./configure --prefix="$prefix" --libdir="$prefix/lib" \
  --enable-shared --disable-static --enable-pic --disable-programs --disable-doc --disable-debug \
  --disable-autodetect --enable-zlib --enable-vaapi --enable-libdrm --enable-libdav1d)
grep -q '^#define CONFIG_GPL 0' "$src/ffmpeg/config.h" || { echo "FFmpeg is configured as GPL; refusing." >&2; exit 1; }
grep -q '^#define CONFIG_NONFREE 0' "$src/ffmpeg/config.h" || { echo "FFmpeg is configured as non-free; refusing." >&2; exit 1; }
grep -q 'FFMPEG_LICENSE "LGPL version 2.1 or later"' "$src/ffmpeg/config.h" || { echo "FFmpeg is not LGPL 2.1+; refusing." >&2; exit 1; }
if ! $configure_only; then make -C "$src/ffmpeg" -j"$jobs" && make -C "$src/ffmpeg" install; fi

# --- FFmpeg for GStreamer (LGPL-2.1+) ------------------------------------------------------
# The web player decodes through the system's gst-libav plugin, linked against the system FFmpeg
# (often a GPL build, which the AppImage must not carry). The AppImage swaps in an LGPL FFmpeg with
# the same sonames; when the plugin wants another libavcodec than the one above, that release is
# built here too, with the same LGPL options.
gst_prefix="/tmp/anix-libmpv-gstprefix-$(id -u)"
gst_version=""
for dir in /usr/lib64/gstreamer-1.0 "/usr/lib/$(uname -m)-linux-gnu/gstreamer-1.0"; do
  if [ -f "$dir/libgstlibav.so" ]; then
    gst_major="$(readelf -d "$dir/libgstlibav.so" | sed -n 's/.*\[libavcodec\.so\.\([0-9]*\)\].*/\1/p')"
    gst_version="$(gst_ffmpeg_for "$gst_major")"
    break
  fi
done
if [ -n "$gst_version" ]; then
  step "FFmpeg $gst_version for GStreamer's libav plugin"
  mkdir -p "$root/prefix-gst"; ln -sfn "$root/prefix-gst" "$gst_prefix"
  fetch "https://ffmpeg.org/releases/ffmpeg-$gst_version.tar.xz" "ffmpeg-$gst_version.tar.xz"
  unpack "ffmpeg-$gst_version.tar.xz" "ffmpeg-$gst_version"
  (cd "$src/ffmpeg-$gst_version" && ./configure --prefix="$gst_prefix" --libdir="$gst_prefix/lib" \
    --enable-shared --disable-static --enable-pic --disable-programs --disable-doc --disable-debug \
    --disable-autodetect --enable-zlib)
  config="$src/ffmpeg-$gst_version/config.h"
  grep -q '^#define CONFIG_GPL 0' "$config" || { echo "FFmpeg $gst_version is configured as GPL; refusing." >&2; exit 1; }
  grep -q '^#define CONFIG_NONFREE 0' "$config" || { echo "FFmpeg $gst_version is configured as non-free; refusing." >&2; exit 1; }
  if ! $configure_only; then make -C "$src/ffmpeg-$gst_version" -j"$jobs" && make -C "$src/ffmpeg-$gst_version" install; fi
fi

# --- mpv (LGPL-2.1+ with -Dgpl=false) -----------------------------------------------------
step "mpv $MPV_TAG"
clone https://github.com/mpv-player/mpv.git "$MPV_TAG" mpv
if $configure_only && [ ! -f "$prefix/lib/pkgconfig/libavcodec.pc" ]; then
  echo "mpv needs the libraries above installed; run without --configure-only to configure it."
else
  meson_lib "$src/mpv" -Dgpl=false -Dlibmpv=true -Dcplayer=false \
    -Dlua=disabled -Djavascript=disabled -Dlibarchive=disabled -Dlibbluray=disabled -Ddvdnav=disabled \
    -Dcdda=disabled -Duchardet=disabled -Drubberband=disabled -Dvapoursynth=disabled -Dzimg=disabled \
    -Dx11=disabled -Dwayland=auto -Degl=enabled -Dgl=enabled -Dvaapi=enabled -Dvulkan=disabled \
    -Dpipewire=auto -Dpulse=enabled -Dalsa=enabled -Dlcms2=enabled \
    -Dmanpage-build=disabled -Dhtml-build=disabled -Dpdf-build=disabled
fi

if $configure_only; then
  echo; echo "Configure steps passed in $(( $(date +%s) - t0 ))s."; exit 0
fi

# --- collect and check --------------------------------------------------------------------
step "collect into $out/lib/anix"
rm -rf "$out"; mkdir -p "$out/lib/anix" "$out/share/licenses"
for lib in libmpv.so.* libavcodec.so.* libavformat.so.* libavutil.so.* libavfilter.so.* libavdevice.so.* \
  libswresample.so.* libswscale.so.* libplacebo.so.* libass.so.* libdisplay-info.so.*; do
  for f in "$prefix"/lib/$lib; do [ -e "$f" ] && cp -P "$f" "$out/lib/anix/"; done
done
if [ -n "$gst_version" ]; then
  for lib in libavcodec.so.* libavformat.so.* libavutil.so.* libavfilter.so.* libswresample.so.* libswscale.so.*; do
    for f in "$gst_prefix"/lib/$lib; do [ -e "$f" ] && cp -P "$f" "$out/lib/anix/"; done
  done
  cp "$src/ffmpeg-$gst_version/COPYING.LGPLv2.1" "$out/share/licenses/FFmpeg-$gst_version-LGPL-2.1.txt"
fi
for f in "$out"/lib/anix/*.so*; do
  [ -L "$f" ] && continue
  patchelf --set-rpath '$ORIGIN' "$f"
  strip --strip-unneeded "$f"
done
cp "$src/ffmpeg/COPYING.LGPLv2.1" "$out/share/licenses/FFmpeg-LGPL-2.1.txt"
cp "$src/mpv/LICENSE.LGPL" "$out/share/licenses/mpv-LGPL-2.1.txt"
cp "$src/libplacebo/LICENSE" "$out/share/licenses/libplacebo-LGPL-2.1.txt"
cp "$src/libass/COPYING" "$out/share/licenses/libass-ISC.txt"

python3 -I - "$out/lib/anix" <<'PY'
import ctypes, glob, os, sys
d = sys.argv[1]
licenses = []
for path in sorted(p for p in glob.glob(f"{d}/libavcodec.so.*") if os.path.basename(p).count(".") == 2):
    av = ctypes.CDLL(path)
    av.avcodec_license.restype = ctypes.c_char_p
    licenses.append(av.avcodec_license().decode())
license = " / ".join(licenses)
mpv = ctypes.CDLL(f"{d}/libmpv.so.2")
mpv.mpv_create.restype = ctypes.c_void_p
mpv.mpv_get_property_string.restype = ctypes.c_void_p
mpv.mpv_get_property_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
mpv.mpv_initialize.argtypes = [ctypes.c_void_p]
mpv.mpv_set_option_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
mpv.mpv_terminate_destroy.argtypes = [ctypes.c_void_p]
h = mpv.mpv_create()
mpv.mpv_set_option_string(h, b"vo", b"null"); mpv.mpv_set_option_string(h, b"ao", b"null")
assert mpv.mpv_initialize(h) >= 0, "mpv_initialize failed"
config = ctypes.string_at(mpv.mpv_get_property_string(h, b"mpv-configuration")).decode()
mpv.mpv_terminate_destroy(h)
print("FFmpeg licence:", license)
print("mpv configuration:", config)
if not licenses or any("LGPL" not in l for l in licenses) or "gpl=false" not in config.replace(" ", ""):
    sys.exit("Not an LGPL build; refusing.")
open(f"{d}/BUILD-LICENSE.txt", "w").write(f"FFmpeg: {license}\nmpv: {config}\n")
PY
if ldd "$out/lib/anix/libmpv.so.2" | grep -E 'libav|libsw|libplacebo|libass' | grep -v "$out/lib/anix"; then
  echo "libmpv resolves some of its libraries outside $out/lib/anix (above)." >&2; exit 1
fi
if grep -rla -e "$HOME" -e "$(id -un)" "$out"; then
  echo "The libraries contain this machine's home folder or user name (files above)." >&2; exit 1
fi
echo; echo "Done in $(( $(date +%s) - t0 ))s: $out/lib/anix"
