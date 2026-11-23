#!/usr/bin/env bash
set -euo pipefail

release_tag=${1:?Usage: check-release-version.sh vMAJOR.MINOR.PATCH}
if [[ ! "$release_tag" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
    echo "Release tag must match vMAJOR.MINOR.PATCH: $release_tag" >&2
    exit 1
fi
expected_version=${BASH_REMATCH[1]}

python3 - "$expected_version" <<'PY'
import json
import subprocess
import sys

expected = sys.argv[1]
checks = [
    ("Cargo.toml", {"gpui-pass-core"}),
    ("desktop/Cargo.toml", {"gpui-pass", "gpui-pass-core"}),
]

for manifest, expected_packages in checks:
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            manifest,
            "--format-version",
            "1",
            "--locked",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    packages = {
        package["name"]: package["version"]
        for package in json.loads(result.stdout)["packages"]
        if package["name"] in expected_packages
    }
    if packages.keys() != expected_packages:
        missing = expected_packages - packages.keys()
        print(
            f"{manifest}: missing expected packages in Cargo metadata: {missing}",
            file=sys.stderr,
        )
        sys.exit(1)
    mismatches = {
        name: version for name, version in packages.items() if version != expected
    }
    if mismatches:
        details = ", ".join(f"{name}={version}" for name, version in mismatches.items())
        print(
            f"{manifest}: package versions ({details}) do not match v{expected}",
            file=sys.stderr,
        )
        sys.exit(1)

print(f"Release tag v{expected} matches both Cargo manifests and lockfiles.")
PY
