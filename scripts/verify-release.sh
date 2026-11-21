#!/usr/bin/env bash
set -euo pipefail

archive=${1:?usage: verify-release.sh ARCHIVE CHECKSUM}
checksum=${2:?usage: verify-release.sh ARCHIVE CHECKSUM}
archive=$(realpath "$archive")
checksum=$(realpath "$checksum")

archive_dir=$(dirname "$archive")
archive_name=$(basename "$archive")
checksum_name=$(basename "$checksum")
workdir=$(mktemp -d "${TMPDIR:-/tmp}/gpui-pass-release.XXXXXX")
trap 'rm -rf "$workdir"' EXIT

printf 'Verifying checksum: %s\n' "$archive_name"
(
    cd "$archive_dir"
    sha256sum --check -- "$checksum_name"
)

tar -tzf "$archive" | grep -Fx 'gpui-pass' >/dev/null
tar -tzf "$archive" | grep -Fx 'INSTALL.txt' >/dev/null

tar -xzf "$archive" --directory "$workdir" --no-same-owner
install_root="$workdir/install"
install -d "$install_root"
install -m 755 "$workdir/gpui-pass" "$install_root/gpui-pass"
install -m 644 "$workdir/INSTALL.txt" "$install_root/INSTALL.txt"

printf 'Verifying installed binary: %s\n' "$install_root/gpui-pass"
"$install_root/gpui-pass" --help >/dev/null

config=$(mktemp -d "$workdir/config.XXXXXX")
set +e
timeout --signal=TERM 15s xvfb-run -a env \
    XDG_CONFIG_HOME="$config" \
    "$install_root/gpui-pass" --demo
status=$?
set -e
if [[ $status -ne 124 ]]; then
    printf 'Demo launch exited unexpectedly with status %s\n' "$status" >&2
    exit "$status"
fi

printf 'Release artifact verification passed.\n'
