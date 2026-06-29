#!/usr/bin/env bash
# P3-24: permissive-only license audit for ui/ runtime dependencies (package.json dependencies).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UI_DIR="${ROOT}/ui"

# SPDX identifiers allowed for shipped UI runtime deps (see docs/licensing.md).
# OFL-1.1: embedded fonts via @fontsource/* only.
export ALLOW_RE='^(Apache-2.0|MIT|BSD-2-Clause|BSD-3-Clause|ISC|0BSD|CC0-1.0|Unlicense|OFL-1.1)$'

cd "${UI_DIR}"

if ! command -v bunx >/dev/null 2>&1; then
  echo "error: bunx required for npm license audit" >&2
  exit 1
fi

bun install --frozen-lockfile >/dev/null

JSON="$(bunx --bun license-checker --production --json --excludePrivatePackages)"

bun -e "
const fs = require('fs');
const allow = new RegExp(process.env.ALLOW_RE);
const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'));
const runtimeDeps = Object.keys(pkg.dependencies || {});
const data = JSON.parse(process.argv[1]);
const bad = [];

for (const name of runtimeDeps) {
  const key = Object.keys(data).find((k) => k.startsWith(name + '@'));
  if (!key) {
    bad.push({ pkg: name, licenses: 'MISSING FROM LOCKFILE' });
    continue;
  }
  const info = data[key];
  const raw = String(info.licenses || 'UNKNOWN').replace(/\\*$/, '');
  const licenses = raw
    .split(/\\s+AND\\s+|\\s+OR\\s+|\\s*,\\s*/)
    .map((s) => s.trim())
    .filter(Boolean);
  const oflFont = name.startsWith('@fontsource/');
  if (!licenses.some((id) => allow.test(id))) {
    bad.push({ pkg: name, licenses: info.licenses });
  } else if (licenses.some((id) => id === 'OFL-1.1') && !oflFont) {
    bad.push({ pkg: name, licenses: 'OFL-1.1 only allowed for @fontsource/*' });
  }
}

if (bad.length) {
  console.error('Non-permissive npm licenses in ui/package.json dependencies:');
  for (const row of bad) {
    console.error('  ' + row.pkg + ': ' + row.licenses);
  }
  process.exit(1);
}
console.log('npm license audit: OK (' + runtimeDeps.length + ' runtime dependencies)');
" "${JSON}"