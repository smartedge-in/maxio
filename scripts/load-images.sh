#!/usr/bin/env bash
# P3-55: load exported image tarballs into a private container registry.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DOCKER="${DOCKER:-docker}"
REGISTRY="${REGISTRY:-}"
INPUT_DIR="${1:-${ROOT}/dist/offline-images}"
DRY_RUN="${DRY_RUN:-0}"

usage() {
  cat <<'EOF'
Usage: REGISTRY=<host[:port]/path> scripts/load-images.sh [images-dir]

Loads docker save tarballs from an offline image pack and pushes them to a private
registry. Reads images.txt when present; otherwise loads every *.tar in the directory.

Environment:
  REGISTRY   Required. Private registry host (e.g. registry.internal:5000 or
             harbor.example.com/maxio).
  DRY_RUN=1  Print actions without docker load/tag/push.

Example:
  REGISTRY=registry.internal:5000/maxio bash scripts/load-images.sh dist/offline-images
EOF
}

if [[ -z "$REGISTRY" ]]; then
  usage >&2
  echo "error: REGISTRY is required" >&2
  exit 1
fi

if [[ ! -d "$INPUT_DIR" ]]; then
  echo "error: images directory not found: ${INPUT_DIR}" >&2
  exit 1
fi

if ! command -v "$DOCKER" >/dev/null 2>&1; then
  echo "error: docker not found" >&2
  exit 1
fi

REGISTRY="${REGISTRY%/}"

run() {
  if [[ "$DRY_RUN" == "1" ]]; then
    printf '[dry-run] %q\n' "$*"
  else
    "$@"
  fi
}

load_and_push() {
  local archive="$1"
  local source_ref="$2"

  echo "==> Loading ${archive}"
  run "$DOCKER" load -i "$archive"

  local base="${source_ref%%:*}"
  local tag="${source_ref##*:}"
  local target="${REGISTRY}/${base}:${tag}"

  echo "==> Pushing ${target}"
  run "$DOCKER" tag "$source_ref" "$target"
  run "$DOCKER" push "$target"
  echo "    published ${target}"
}

MANIFEST="${INPUT_DIR}/images.txt"
if [[ -f "$MANIFEST" ]]; then
  ARCHIVE=""
  SOURCE_REF=""
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ "$line" =~ ^# ]] && continue
    [[ -z "${line// }" ]] && continue
    key="${line%%=*}"
    value="${line#*=}"
    case "$key" in
      archive) ARCHIVE="$value" ;;
      image) SOURCE_REF="$value" ;;
    esac
    if [[ -n "$ARCHIVE" && -n "$SOURCE_REF" ]]; then
      load_and_push "${INPUT_DIR}/${ARCHIVE}" "$SOURCE_REF"
      ARCHIVE=""
      SOURCE_REF=""
    fi
  done < "$MANIFEST"
else
  shopt -s nullglob
  archives=("${INPUT_DIR}"/*.tar)
  if [[ ${#archives[@]} -eq 0 ]]; then
    echo "error: no *.tar archives found in ${INPUT_DIR}" >&2
    exit 1
  fi
  for archive in "${archives[@]}"; do
    echo "==> Loading ${archive}"
    if [[ "$DRY_RUN" == "1" ]]; then
      printf '[dry-run] %q load -i %q\n' "$DOCKER" "$archive"
      continue
    fi
    loaded="$("$DOCKER" load -i "$archive" | awk '/Loaded image:/ {print $3; exit}')"
    if [[ -z "$loaded" ]]; then
      echo "error: could not determine loaded image ref from ${archive}" >&2
      exit 1
    fi
    base="${loaded%%:*}"
    tag="${loaded##*:}"
    target="${REGISTRY}/${base}:${tag}"
    run "$DOCKER" tag "$loaded" "$target"
    run "$DOCKER" push "$target"
    echo "    published ${target}"
  done
fi

echo "==> Private registry ingest complete (${REGISTRY})"