#!/usr/bin/env bash
# P3-54: build offline release bundle for bare-metal / jump-host install.
# Produces a versioned tarball with binaries, checksums, license files, and SBOM.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
LOCAL_BIN="${LOCAL_BIN:-$HOME/.local/bin}"
export PATH="${CARGO_HOME}/bin:${LOCAL_BIN}:${PATH}"

CARGO="$(command -v cargo 2>/dev/null || echo "${CARGO_HOME}/bin/cargo")"
TRIVY="$(command -v trivy 2>/dev/null || echo "${LOCAL_BIN}/trivy")"

OUT_DIR="${OUT_DIR:-${ROOT}/dist/offline-bundle}"
SKIP_BUILD="${SKIP_BUILD:-0}"
SKIP_SBOM="${SKIP_SBOM:-0}"

if [[ ! -f VERSION ]]; then
  echo "error: VERSION file not found at ${ROOT}/VERSION" >&2
  exit 1
fi

VERSION="$(tr -d '[:space:]' < VERSION)"
ARCH_RAW="$(uname -m)"
case "$ARCH_RAW" in
  x86_64) ARCH=amd64 ;;
  aarch64|arm64) ARCH=arm64 ;;
  *)
    echo "error: unsupported architecture: ${ARCH_RAW} (expected x86_64 or aarch64)" >&2
    exit 1
    ;;
esac

BUNDLE_NAME="maxio-offline-${VERSION}-linux-${ARCH}"
STAGING="${OUT_DIR}/${BUNDLE_NAME}"
RELEASE_BINS=(maxio maxio-admin maxio-ui)
RELEASE_PKGS=(maxio maxio-admin maxio-ui)

echo "==> P3-54 offline release bundle"
echo "    version: ${VERSION}"
echo "    arch:    linux/${ARCH}"
echo "    output:  ${OUT_DIR}"

if [[ "$SKIP_BUILD" != "1" ]]; then
  echo "==> Building release binaries"
  if command -v bun >/dev/null 2>&1 && [[ "${SKIP_FRONTEND:-}" != "1" ]]; then
    (cd ui && bun install --frozen-lockfile && bun run build)
    env -u SKIP_FRONTEND "$CARGO" build --release --locked \
      $(printf ' -p %s' "${RELEASE_PKGS[@]}")
  else
    SKIP_FRONTEND=1 "$CARGO" build --release --locked \
      $(printf ' -p %s' "${RELEASE_PKGS[@]}")
  fi
fi

for bin in "${RELEASE_BINS[@]}"; do
  if [[ ! -x "target/release/${bin}" ]]; then
    echo "error: missing release binary target/release/${bin}" >&2
    exit 1
  fi
done

rm -rf "$STAGING"
mkdir -p "${STAGING}/bin"

for bin in "${RELEASE_BINS[@]}"; do
  install -m 0755 "target/release/${bin}" "${STAGING}/bin/${bin}"
done

cp LICENSE "${STAGING}/LICENSE"

echo "==> Generating LICENSES.txt from Cargo metadata"
python3 - <<'PY' > "${STAGING}/LICENSES.txt"
import json
import subprocess
import sys

data = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        text=True,
    )
)
rows = []
seen = set()
for pkg in sorted(data.get("packages", []), key=lambda p: (p["name"], p["version"])):
    key = (pkg["name"], pkg["version"])
    if key in seen:
        continue
    seen.add(key)
    license_id = pkg.get("license") or "UNKNOWN"
    rows.append(f"{pkg['name']} {pkg['version']}: {license_id}")

print("MaxIO third-party Rust dependency licenses (from cargo metadata)")
print("=" * 72)
print("MaxIO itself is licensed under Apache-2.0 — see LICENSE in this bundle.")
print("Embedded UI runtime licenses are audited separately (docs/licensing.md).")
print()
for row in rows:
    print(row)
PY

if [[ "$SKIP_SBOM" != "1" ]]; then
  echo "==> Generating CycloneDX SBOM (trivy)"
  if ! command -v "$TRIVY" >/dev/null 2>&1; then
    echo "error: trivy not found. Run 'make install-tools' or set SKIP_SBOM=1." >&2
    exit 1
  fi
  TRIVY_CACHE_DIR="${TRIVY_CACHE_DIR:-/tmp/maxio-trivy-cache}"
  mkdir -p "$TRIVY_CACHE_DIR"
  "$TRIVY" fs \
    --format cyclonedx \
    --output "${STAGING}/sbom.json" \
    --cache-dir "$TRIVY_CACHE_DIR" \
    .
else
  echo "==> Skipping SBOM generation (SKIP_SBOM=1)"
fi

printf '%s\n' "$VERSION" > "${STAGING}/VERSION"

echo "==> Writing SHA256SUMS"
(
  cd "$STAGING"
  find . -type f ! -name SHA256SUMS | sort | while read -r rel; do
    rel="${rel#./}"
    sha256sum "$rel"
  done
) > "${STAGING}/SHA256SUMS"

mkdir -p "$OUT_DIR"
ARCHIVE="${OUT_DIR}/${BUNDLE_NAME}.tar.gz"
tar -C "$OUT_DIR" -czf "$ARCHIVE" "$BUNDLE_NAME"

(
  cd "$OUT_DIR"
  sha256sum "$(basename "$ARCHIVE")" > "${BUNDLE_NAME}.tar.gz.sha256"
)

echo "==> Bundle ready"
echo "    ${ARCHIVE}"
echo "    ${ARCHIVE}.sha256"
echo ""
echo "Verify before install:"
echo "  cd ${OUT_DIR} && sha256sum -c ${BUNDLE_NAME}.tar.gz.sha256"
echo "  tar -xzf ${BUNDLE_NAME}.tar.gz && cd ${BUNDLE_NAME} && sha256sum -c SHA256SUMS"