#!/usr/bin/env bash
# Sync the repository VERSION file into Cargo.toml and ui/package.json.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ ! -f VERSION ]]; then
  echo "error: VERSION file not found at $ROOT/VERSION" >&2
  exit 1
fi

VERSION="$(tr -d '[:space:]' < VERSION)"

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?(\+[0-9A-Za-z.]+)?$ ]]; then
  echo "error: VERSION must be semantic (MAJOR.MINOR.PATCH): got '$VERSION'" >&2
  exit 1
fi

python3 - "$VERSION" <<'PY'
import json
import pathlib
import re
import sys

version = sys.argv[1]
cargo = pathlib.Path("Cargo.toml")
text = cargo.read_text()
new, n = re.subn(
    r'(\[workspace\.package\]\s*\n(?:[^\[]*\n)*?version = )"[^"]*"',
    rf'\1"{version}"',
    text,
    count=1,
)
if n != 1:
    raise SystemExit("could not update [workspace.package] version in Cargo.toml")
cargo.write_text(new)

ui_pkg = pathlib.Path("ui/package.json")
data = json.loads(ui_pkg.read_text())
data["version"] = version
ui_pkg.write_text(json.dumps(data, indent=2) + "\n")
PY

printf 'synced semantic version %s → Cargo.toml, ui/package.json\n' "$VERSION"