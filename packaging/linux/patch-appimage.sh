#!/bin/bash
# patch-appimage.sh — Post-process an AppImage to fix WebKit portability
#
# This script:
# 1. Extracts the AppImage
# 2. Copies WebKit subprocess executables into the AppDir
# 3. Binary-patches libwebkit/libjavascriptcore to replace /usr with ././
# 4. Replaces AppRun with a custom script that cd's into $APPDIR/usr/
# 5. Removes bundled Wayland libs so host EGL/Wayland ABI stays consistent
# 6. Repacks using appimagetool
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if [ $# -lt 1 ]; then
    echo "Usage: $0 <path-to-AppImage> [output-path]"
    exit 1
fi

APPIMAGE_PATH="$(realpath "$1")"
OUTPUT_PATH="${2:-$APPIMAGE_PATH}"

if [ ! -f "$APPIMAGE_PATH" ]; then
    echo "Error: AppImage not found: $APPIMAGE_PATH"
    exit 1
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

echo "==> Extracting AppImage..."
cd "$WORK_DIR"
chmod +x "$APPIMAGE_PATH"
"$APPIMAGE_PATH" --appimage-extract >/dev/null 2>&1
APPDIR="$WORK_DIR/squashfs-root"

if [ ! -d "$APPDIR" ]; then
    echo "Error: Extraction failed — squashfs-root not found"
    exit 1
fi

# --- Copy WebKit subprocess executables ---
echo "==> Copying WebKit subprocess executables..."

# Find the WebKit lib directory on the build system
WEBKIT_LIB_DIR=""
for candidate in \
    /usr/lib/x86_64-linux-gnu/webkit2gtk-4.1 \
    /usr/lib64/webkit2gtk-4.1 \
    /usr/lib/webkit2gtk-4.1; do
    if [ -d "$candidate" ]; then
        WEBKIT_LIB_DIR="$candidate"
        break
    fi
done

if [ -z "$WEBKIT_LIB_DIR" ]; then
    echo "Error: Could not find WebKit lib directory on build system"
    exit 1
fi

# Determine the destination path inside the AppDir, matching the build system layout
# e.g., /usr/lib/x86_64-linux-gnu/webkit2gtk-4.1 → usr/lib/x86_64-linux-gnu/webkit2gtk-4.1
WEBKIT_REL_DIR="${WEBKIT_LIB_DIR#/}"
WEBKIT_DEST="$APPDIR/$WEBKIT_REL_DIR"
mkdir -p "$WEBKIT_DEST"

for exe in WebKitNetworkProcess WebKitWebProcess; do
    if [ -f "$WEBKIT_LIB_DIR/$exe" ]; then
        cp -v "$WEBKIT_LIB_DIR/$exe" "$WEBKIT_DEST/$exe"
    else
        echo "Warning: $exe not found at $WEBKIT_LIB_DIR/$exe"
    fi
done

# Copy injected bundle if present
BUNDLE_DIR="$WEBKIT_DEST/injected-bundle"
mkdir -p "$BUNDLE_DIR"
if [ -f "$WEBKIT_LIB_DIR/injected-bundle/libwebkit2gtkinjectedbundle.so" ]; then
    cp -v "$WEBKIT_LIB_DIR/injected-bundle/libwebkit2gtkinjectedbundle.so" \
          "$BUNDLE_DIR/libwebkit2gtkinjectedbundle.so"
fi

# --- Binary-patch WebKit shared libraries ---
echo "==> Binary-patching WebKit libraries (replacing /usr with ././)..."
find "$APPDIR/usr/lib" -type f \( -name 'libwebkit2gtk*' -o -name 'libjavascriptcoregtk*' \) | while read -r lib; do
    echo "  Patching: $(basename "$lib")"
    sed -i 's|/usr|././|g' "$lib"
done

# --- Replace AppRun ---
echo "==> Installing custom AppRun..."
rm -f "$APPDIR/AppRun"
cp "$SCRIPT_DIR/AppRun" "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"

# --- Remove ABI-sensitive Wayland libs ---
echo "==> Removing bundled Wayland libraries (use host-provided versions)..."
rm -f \
  "$APPDIR/usr/lib/libwayland-client.so"* \
  "$APPDIR/usr/lib/libwayland-cursor.so"* \
  "$APPDIR/usr/lib/libwayland-egl.so"* \
  "$APPDIR/usr/lib/libwayland-server.so"* || true

# --- Repack with appimagetool ---
echo "==> Downloading appimagetool (if needed)..."
APPIMAGETOOL="$WORK_DIR/appimagetool"
if ! command -v appimagetool &>/dev/null; then
    curl -fsSL -o "$APPIMAGETOOL" \
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$APPIMAGETOOL"
else
    APPIMAGETOOL="$(command -v appimagetool)"
fi

echo "==> Repacking AppImage..."
ARCH=x86_64 "$APPIMAGETOOL" "$APPDIR" "$OUTPUT_PATH" >/dev/null 2>&1

echo "==> Done! Patched AppImage: $OUTPUT_PATH"

# Verify the patch
echo "==> Verifying patch..."
PATCHED_COUNT=$(find "$APPDIR/usr/lib" -type f -name 'libwebkit2gtk*' -exec strings {} \; | grep -c '././' || true)
if [ "$PATCHED_COUNT" -gt 0 ]; then
    echo "  OK: Found $PATCHED_COUNT '././' references in patched libraries"
else
    echo "  WARNING: No '././' references found — patch may not have applied"
fi
