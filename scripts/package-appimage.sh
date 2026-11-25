#!/usr/bin/env bash
set -euo pipefail

binary=${1:-desktop/target/release/gpui-pass}
output=${2:-dist/gpui-pass-linux-x86_64.AppImage}
linuxdeploy=${LINUXDEPLOY:-linuxdeploy}
runtime=${LDAI_RUNTIME_FILE:-}

if [[ -z "$runtime" || ! -f "$runtime" ]]; then
    echo "Set LDAI_RUNTIME_FILE to the verified x86_64 AppImage type 2 runtime." >&2
    exit 1
fi
if [[ ! -x "$binary" ]]; then
    echo "Executable not found: $binary" >&2
    exit 1
fi
if ! command -v "$linuxdeploy" >/dev/null 2>&1; then
    echo "linuxdeploy is required (set LINUXDEPLOY to its executable path)." >&2
    exit 1
fi
if ! command -v linuxdeploy-plugin-appimage >/dev/null 2>&1; then
    echo "linuxdeploy-plugin-appimage is required and must be on PATH." >&2
    exit 1
fi

appdir=$(mktemp -d "${TMPDIR:-/tmp}/gpui-pass.AppDir.XXXXXX")
trap 'rm -rf "$appdir"' EXIT
mkdir -p "$(dirname "$output")"

install -Dm755 "$binary" "$appdir/usr/bin/gpui-pass"

# linuxdeploy gathers the app's shared-library dependencies into the AppDir.
# GnuPG, pinentry, smartcard services, and system graphics drivers stay external.
APPIMAGE_EXTRACT_AND_RUN=1 LDAI_RUNTIME_FILE="$runtime" LDAI_OUTPUT="$output" "$linuxdeploy" \
    --appdir "$appdir" \
    --executable "$appdir/usr/bin/gpui-pass" \
    --desktop-file packaging/dev.gpui-pass.desktop \
    --icon-file assets/gpui-pass.svg \
    --output appimage

if [[ ! -s "$output" ]]; then
    echo "AppImage was not created at $output" >&2
    exit 1
fi
chmod 755 "$output"
