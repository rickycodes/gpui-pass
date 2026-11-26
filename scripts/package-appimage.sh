#!/usr/bin/env bash
set -euo pipefail

die() {
    printf '%s\n' "$*" >&2
    exit 1
}

file_sha256() {
    local digest
    digest=$(sha256sum < "$1") || return
    printf '%s\n' "${digest%% *}"
}

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
binary=${1:-"$repo_root/desktop/target/release/gpui-pass"}
output=${2:-"$repo_root/dist/gpui-pass-linux-x86_64.AppImage"}
[[ -x "$binary" ]] || die "Executable not found: $binary"

for tool in curl sha256sum file; do
    command -v "$tool" >/dev/null || die "Required packaging tool not found: $tool"
done

binary=$(realpath "$binary")
output=$(realpath -m "$output")
cache_home=${XDG_CACHE_HOME:-"$HOME/.cache"}
tool_dir=${APPIMAGE_TOOLS_DIR:-"$cache_home/gpui-pass/appimage-tools"}
tool_dir=$(realpath -m "$tool_dir")
mkdir -p "$tool_dir" "$(dirname "$output")"

fetch_verified() {
    local url=$1 expected=$2 destination=$3 attempt actual
    if [[ -f "$destination" ]] && actual=$(file_sha256 "$destination") \
        && [[ "$actual" == "$expected" ]]; then
        echo "Using verified cached tool: $destination"
        return 0
    fi

    rm -f "$destination"
    for attempt in 1 2 3; do
        if curl -fsSL --retry 2 -o "$destination" "$url"; then
            actual=$(file_sha256 "$destination")
            if [[ "$actual" == "$expected" ]]; then
                echo "Verified download: $destination"
                return 0
            fi
            echo "SHA-256 mismatch for $destination (attempt $attempt/3): expected $expected, got $actual" >&2
        else
            echo "Download failed for $url (attempt $attempt/3)." >&2
        fi
        rm -f "$destination"
        sleep "$attempt"
    done
    die "Could not download a verified copy of $destination."
}

install_appimage_tool() {
    local name=$1 url=$2 expected=$3
    local image="$tool_dir/$name.AppImage"
    fetch_verified "$url" "$expected" "$image"
    chmod 755 "$image"
    ln -sfn "$name.AppImage" "$tool_dir/$name"
}

install_appimage_tool linuxdeploy \
    https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage \
    c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
install_appimage_tool linuxdeploy-plugin-appimage \
    https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/1-alpha-20250213-1/linuxdeploy-plugin-appimage-x86_64.AppImage \
    992d502a248e14ab185448ddf6f6e7d25558cb84d4623c354c3af350c25fccb3
fetch_verified \
    https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64 \
    617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46 \
    "$tool_dir/runtime-x86_64"

appdir=$(mktemp -d "${TMPDIR:-/tmp}/gpui-pass.AppDir.XXXXXX")
trap 'rm -rf "$appdir"' EXIT
install -Dm755 "$binary" "$appdir/usr/bin/gpui-pass"

# Keep host GnuPG, pinentry, smartcard support, and graphics drivers external.
PATH="$tool_dir:$PATH" \
    APPIMAGE_EXTRACT_AND_RUN=1 \
    LDAI_RUNTIME_FILE="$tool_dir/runtime-x86_64" \
    LDAI_OUTPUT="$output" \
    "$tool_dir/linuxdeploy" \
    --appdir "$appdir" \
    --executable "$appdir/usr/bin/gpui-pass" \
    --desktop-file "$repo_root/packaging/dev.gpui-pass.desktop" \
    --icon-file "$repo_root/assets/gpui-pass.svg" \
    --output appimage

[[ -s "$output" ]] || die "AppImage was not created at $output"
chmod 755 "$output"

echo "Smoke-testing packaged binary."
APPIMAGE_EXTRACT_AND_RUN=1 "$output" --help >/dev/null
echo "AppImage package smoke test passed: $output"
