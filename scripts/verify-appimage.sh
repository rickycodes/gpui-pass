#!/usr/bin/env bash
set -euo pipefail

appimage=${1:?usage: verify-appimage.sh APPIMAGE CHECKSUM}
checksum=${2:?usage: verify-appimage.sh APPIMAGE CHECKSUM}
appimage=$(realpath "$appimage")
checksum=$(realpath "$checksum")
workdir=$(mktemp -d "${TMPDIR:-/tmp}/gpui-pass-appimage.XXXXXX")
trap 'rm -rf "$workdir"' EXIT

printf 'Verifying checksum: %s\n' "$(basename "$appimage")"
(
    cd "$(dirname "$appimage")"
    sha256sum --check "$(basename "$checksum")"
)

chmod 755 "$appimage"
(
    cd "$workdir"
    "$appimage" --appimage-extract >/dev/null
)
appdir="$workdir/squashfs-root"
test -x "$appdir/AppRun"
test -x "$appdir/usr/bin/gpui-pass"
test -f "$appdir/usr/share/applications/dev.gpui-pass.desktop"
test -f "$appdir/usr/share/icons/hicolor/scalable/apps/gpui-pass.svg"
grep -Fxq 'Icon=gpui-pass' "$appdir/usr/share/applications/dev.gpui-pass.desktop"

printf 'Verifying AppImage command line.\n'
APPIMAGE_EXTRACT_AND_RUN=1 "$appimage" --help >/dev/null

printf 'Launching AppImage demo under Xvfb.\n'
config="$workdir/config"
mkdir -p "$config"
env_args=()
if [[ -n "${VK_ICD_FILENAMES:-}" ]]; then
    env_args+=("VK_ICD_FILENAMES=$VK_ICD_FILENAMES")
fi
set +e
timeout --signal=TERM 15s xvfb-run -a env \
    APPIMAGE_EXTRACT_AND_RUN=1 \
    XDG_CONFIG_HOME="$config" \
    "${env_args[@]}" \
    "$appimage" --demo
status=$?
set -e
if [[ $status -ne 124 ]]; then
    printf 'AppImage demo exited unexpectedly with status %s\n' "$status" >&2
    exit "$status"
fi

printf 'AppImage verification passed.\n'
