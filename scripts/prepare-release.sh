#!/usr/bin/env bash
set -euo pipefail

release_tag=${1:?Usage: prepare-release.sh vMAJOR.MINOR.PATCH}
if [[ ! "$release_tag" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
    echo "Release version must match vMAJOR.MINOR.PATCH: $release_tag" >&2
    exit 1
fi
version=${BASH_REMATCH[1]}

if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
    echo "Commit or stash tracked changes before preparing a release." >&2
    exit 1
fi
if git rev-parse --verify --quiet "refs/tags/$release_tag" >/dev/null; then
    echo "Local tag $release_tag already exists." >&2
    exit 1
fi

python3 - "$version" <<'PY'
import re
import sys
from pathlib import Path

version = sys.argv[1]
for manifest in (Path("Cargo.toml"), Path("desktop/Cargo.toml")):
    text = manifest.read_text()
    match = re.search(r"(?ms)^\[package\]\s*(.*?)(?=^\[|\Z)", text)
    if match is None:
        raise SystemExit(f"Could not find [package] section in {manifest}")
    section = match.group(1)
    updated, count = re.subn(
        r'(?m)^(version\s*=\s*)"[^"\n]+"',
        rf'\g<1>"{version}"',
        section,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"Could not find package version in {manifest}")
    manifest.write_text(text[: match.start(1)] + updated + text[match.end(1) :])
PY

cargo update --manifest-path Cargo.toml --offline gpui-pass-core
cargo update --manifest-path desktop/Cargo.toml --offline gpui-pass gpui-pass-core
bash scripts/check-release-version.sh "$release_tag"

git add -- Cargo.toml Cargo.lock desktop/Cargo.toml desktop/Cargo.lock
git diff --cached --check
git commit -m "chore(release): prepare $release_tag"
