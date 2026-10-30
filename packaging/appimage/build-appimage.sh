#!/usr/bin/env bash
# Builds doknyu-x86_64.AppImage from an already built release binary.
#
#   cargo build --release && packaging/appimage/build-appimage.sh
#
# Steps: assemble an AppDir (binary, .desktop, icons, metainfo), let `linuxdeploy` validate it and
# bundle any shared libraries the binary links to, then pack it with `appimagetool`.
#
# What is NOT bundled, on purpose: Wayland, X11, xkbcommon, Vulkan, GL/EGL and the GPU drivers.
# iced/wgpu/winit load them at run time (dlopen), so they come from the host system, which is the
# only place that has the driver matching the user's GPU. linuxdeploy also keeps libGL, libEGL,
# libvulkan, libdrm and similar on its own excludelist. Do not add them back (no
# `--library libvulkan...`): a bundled copy breaks the host's drivers.
#
# The tool downloads happen only when the tools are missing. In CI nothing is cached, so they are
# fetched there, pinned to a release and checked against a SHA-256. Locally, set LINUXDEPLOY and
# APPIMAGETOOL to existing binaries, or put them in $TOOLS_DIR (default: packaging/appimage/.tools).
#
# AppImages need FUSE 2 to run; the tools here are run with --appimage-extract-and-run so that the
# build does not need it (CI containers, for instance, have no /dev/fuse).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ID="io.github.rafaelprofmgz.Doknyu"
ARCH="x86_64"

# Pinned tools. To bump: change the tag and the checksum together (GitHub shows the SHA-256 of each
# release asset, or run `sha256sum` on a download made from the official release page).
LINUXDEPLOY_TAG="1-alpha-20251107-1"
LINUXDEPLOY_SHA256="c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/${LINUXDEPLOY_TAG}/linuxdeploy-${ARCH}.AppImage"
APPIMAGETOOL_TAG="1.9.1"
APPIMAGETOOL_SHA256="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/${APPIMAGETOOL_TAG}/appimagetool-${ARCH}.AppImage"

BIN="${BIN:-$ROOT/target/release/doknyu}"
OUT_DIR="${OUT_DIR:-$ROOT/target/appimage}"
TOOLS_DIR="${TOOLS_DIR:-$ROOT/packaging/appimage/.tools}"
APPDIR="$OUT_DIR/AppDir"
OUTPUT="$OUT_DIR/doknyu-${ARCH}.AppImage"

# fetch_tool NAME URL SHA256 -> prints the path of the tool, downloading it if it is not there yet.
fetch_tool() {
    local name="$1" url="$2" sha="$3" file="$TOOLS_DIR/$1-${ARCH}.AppImage"
    if [[ ! -f "$file" ]]; then
        mkdir -p "$TOOLS_DIR"
        echo "Downloading $name from $url" >&2
        curl --fail --location --silent --show-error --output "$file.part" "$url"
        mv "$file.part" "$file"
    fi
    # Checked on every run, so a file left in $TOOLS_DIR cannot be swapped silently.
    echo "$sha  $file" | sha256sum --check --status \
        || { echo "error: checksum mismatch for $file" >&2; exit 1; }
    chmod +x "$file"
    echo "$file"
}

[[ -x "$BIN" ]] || { echo "error: $BIN not found; run 'cargo build --release' first" >&2; exit 1; }

LINUXDEPLOY="${LINUXDEPLOY:-$(fetch_tool linuxdeploy "$LINUXDEPLOY_URL" "$LINUXDEPLOY_SHA256")}"
APPIMAGETOOL="${APPIMAGETOOL:-$(fetch_tool appimagetool "$APPIMAGETOOL_URL" "$APPIMAGETOOL_SHA256")}"

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/metainfo" "$APPDIR/usr/share/doc/doknyu"

install -m 755 "$BIN" "$APPDIR/usr/bin/doknyu"
install -m 644 "$ROOT/assets/$ID.metainfo.xml" "$APPDIR/usr/share/metainfo/"
# Licenses travel with the binary: the OFL (IBM Plex) and ISC (Lucide) fonts embedded in it
# require their license text to accompany the distribution.
install -m 644 "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/assets/fonts/LICENSES.md" \
    "$APPDIR/usr/share/doc/doknyu/"

# Icons: linuxdeploy copies each --icon-file into usr/share/icons/hicolor/<size>/apps/ (it reads
# the size from the image) and the SVG into scalable/apps/. The one in the AppDir root is
# created for the .DirIcon, which the file managers use.
icon_args=()
for png in "$ROOT"/assets/icons/hicolor/*/apps/$ID.png; do
    icon_args+=(--icon-file "$png")
done
icon_args+=(--icon-file "$ROOT/assets/icons/$ID.svg")

export APPIMAGE_EXTRACT_AND_RUN=1
# linuxdeploy is told the AppDir layout; no plugin is used (no Qt/GTK), so only the .desktop,
# the icons and the shared-library check are done. It does NOT create the AppImage here.
"$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/doknyu" \
    --desktop-file "$ROOT/assets/$ID.desktop" \
    "${icon_args[@]}"

# Fail early, with a clear message, if linuxdeploy did not leave a valid AppDir.
[[ -e "$APPDIR/AppRun" ]] || { echo "error: $APPDIR/AppRun is missing" >&2; exit 1; }
[[ -e "$APPDIR/$ID.desktop" ]] || { echo "error: $APPDIR/$ID.desktop is missing" >&2; exit 1; }
[[ -e "$APPDIR/$ID.svg" || -e "$APPDIR/$ID.png" ]] \
    || { echo "error: no icon ($ID.svg or $ID.png) in the root of $APPDIR" >&2; exit 1; }

# AppRun: linuxdeploy creates a symlink to the binary, which is enough because nothing needs
# LD_LIBRARY_PATH or env vars. Packing then adds the type 2 runtime (embedded in appimagetool).
ARCH="$ARCH" "$APPIMAGETOOL" "$APPDIR" "$OUTPUT"

echo "Built $OUTPUT"
