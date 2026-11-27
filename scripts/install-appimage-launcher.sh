#!/usr/bin/env bash
set -euo pipefail

appimage=${1:?usage: install-appimage-launcher.sh APPIMAGE}
if [[ ! -f "$appimage" || ! -x "$appimage" ]]; then
    printf 'AppImage must be an executable file: %s\n' "$appimage" >&2
    exit 1
fi
appimage=$(realpath -- "$appimage")
if [[ "$appimage" == *[$'\n\r=']* ]]; then
    printf 'AppImage path cannot contain a line break or equals sign.\n' >&2
    exit 1
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
launcher="$data_home/applications/dev.gpui-pass.desktop"

# Quote a single Exec argument, then escape it for the desktop file itself.
exec_path=${appimage//\\/\\\\}
exec_path=${exec_path//\"/\\\"}
exec_path=${exec_path//\$/\\\$}
exec_path=${exec_path//\`/\\\`}
exec_path=${exec_path//%/%%}
exec_path=${exec_path//\\/\\\\}

install -Dm644 "$repo_root/assets/gpui-pass.svg" \
    "$data_home/icons/hicolor/scalable/apps/gpui-pass.svg"
mkdir -p -- "$(dirname -- "$launcher")"
staged=$(mktemp -- "$launcher.XXXXXX")
trap 'rm -f -- "$staged"' EXIT
while IFS= read -r line; do
    if [[ "$line" == Exec=* ]]; then
        printf 'Exec="%s"\n' "$exec_path"
    else
        printf '%s\n' "$line"
    fi
done < "$repo_root/packaging/dev.gpui-pass.desktop" > "$staged"
chmod 644 "$staged"
mv -fT -- "$staged" "$launcher"

printf 'Registered %s\n' "$launcher"
