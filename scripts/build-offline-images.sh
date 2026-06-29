#!/usr/bin/env bash
# P3-55: build and export container images for airgapped private-registry ingest.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DOCKER="${DOCKER:-docker}"
OUT_DIR="${OUT_DIR:-${ROOT}/dist/offline-images}"
SKIP_BUILD="${SKIP_BUILD:-0}"

if [[ ! -f VERSION ]]; then
  echo "error: VERSION file not found at ${ROOT}/VERSION" >&2
  exit 1
fi

VERSION="$(tr -d '[:space:]' < VERSION)"
IMAGE_TAG="v${VERSION}"
ARCH_RAW="$(uname -m)"
case "$ARCH_RAW" in
  x86_64) ARCH=amd64; PLATFORM=linux/amd64 ;;
  aarch64|arm64) ARCH=arm64; PLATFORM=linux/arm64 ;;
  *)
    echo "error: unsupported architecture: ${ARCH_RAW}" >&2
    exit 1
    ;;
esac

# Images exported for offline ingest (maxio server + standalone UI tier).
IMAGES=(
  "maxio"
  "maxio-ui"
)

echo "==> P3-55 offline container image pack"
echo "    version:  ${IMAGE_TAG}"
echo "    platform: ${PLATFORM}"
echo "    output:   ${OUT_DIR}"

if ! command -v "$DOCKER" >/dev/null 2>&1; then
  echo "error: docker not found" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"

MANIFEST="${OUT_DIR}/images.txt"
cat > "$MANIFEST" <<EOF
# MaxIO offline image manifest (P3-55)
# Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)
# Load with: scripts/load-images.sh REGISTRY=registry.internal:5000/maxio

EOF

for IMAGE_NAME in "${IMAGES[@]}"; do
  LOCAL_REF="${IMAGE_NAME}:${IMAGE_TAG}"
  TAR_NAME="${IMAGE_NAME}-${IMAGE_TAG}-linux-${ARCH}.tar"
  TAR_PATH="${OUT_DIR}/${TAR_NAME}"
  DOCKERFILE="Dockerfile"
  if [[ "$IMAGE_NAME" == "maxio-ui" ]]; then
    DOCKERFILE="Dockerfile.ui"
  fi

  echo "==> Image: ${LOCAL_REF} (${DOCKERFILE})"

  if [[ "$SKIP_BUILD" != "1" ]]; then
    echo "    Building Docker image"
    "$DOCKER" build \
      --build-arg MAXIO_VERSION="${VERSION}" \
      -t "${LOCAL_REF}" \
      -f "${DOCKERFILE}" \
      .
  fi

  echo "    Exporting to ${TAR_PATH}"
  "$DOCKER" save -o "$TAR_PATH" "${LOCAL_REF}"

  DIGEST="$("$DOCKER" image inspect "${LOCAL_REF}" --format '{{index .RepoDigests 0}}' 2>/dev/null || true)"
  if [[ -z "$DIGEST" ]]; then
    IMAGE_ID="$("$DOCKER" image inspect "${LOCAL_REF}" --format '{{.Id}}')"
    DIGEST="id:${IMAGE_ID}"
  fi

  cat >> "$MANIFEST" <<EOF
image=${LOCAL_REF}
platform=${PLATFORM}
archive=${TAR_NAME}
digest=${DIGEST}

EOF

  (
    cd "$OUT_DIR"
    sha256sum "$TAR_NAME" > "${TAR_NAME}.sha256"
  )
done

echo "==> Image pack ready"
echo "    ${MANIFEST}"
echo ""
echo "Ingest to private registry:"
echo "  REGISTRY=registry.internal:5000/maxio bash scripts/load-images.sh ${OUT_DIR}"