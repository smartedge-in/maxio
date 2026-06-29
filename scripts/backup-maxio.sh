#!/usr/bin/env bash
# P3-48: backup MaxIO data directory with checksum verification artifacts.
set -euo pipefail

DATA_DIR="${MAXIO_DATA_DIR:-${1:-}}"
BACKUP_ROOT="${MAXIO_BACKUP_ROOT:-${2:-./backups}}"
RETENTION_DAYS="${MAXIO_BACKUP_RETENTION_DAYS:-0}"
LABEL="${MAXIO_BACKUP_LABEL:-}"

usage() {
  cat <<'EOF'
Usage: backup-maxio.sh <data_dir> [backup_root]

Creates a timestamped tar archive of the MaxIO data directory including:
  - buckets/ object payload and metadata
  - .maxio-keys.json (SSE-S3 keyring)
  - .maxio-credentials.json (if present)
  - .maxio-metadata.db (if metadata index enabled)

Outputs:
  <backup_root>/maxio-backup-YYYYMMDD-HHMMSS.tar.gz
  <backup_root>/maxio-backup-YYYYMMDD-HHMMSS.tar.gz.sha256
  <backup_root>/maxio-backup-YYYYMMDD-HHMMSS/SHA256SUMS (contents manifest)

Environment:
  MAXIO_DATA_DIR              Override data directory positional argument
  MAXIO_BACKUP_ROOT           Override backup root (default: ./backups)
  MAXIO_BACKUP_RETENTION_DAYS Prune backups older than N days (0 = keep all)
  MAXIO_BACKUP_LABEL          Optional suffix appended to archive name

Restore drill:
  1. Stop MaxIO
  2. Verify: sha256sum -c <archive>.sha256
  3. Extract to a staging path and compare SHA256SUMS inside the archive
  4. Replace production data_dir and restart
EOF
}

if [[ -z "$DATA_DIR" ]]; then
  usage >&2
  echo "error: data directory is required" >&2
  exit 1
fi

if [[ ! -d "$DATA_DIR" ]]; then
  echo "error: data directory not found: ${DATA_DIR}" >&2
  exit 1
fi

DATA_DIR="$(cd "$DATA_DIR" && pwd)"
mkdir -p "$BACKUP_ROOT"
BACKUP_ROOT="$(cd "$BACKUP_ROOT" && pwd)"

STAMP="$(date -u +%Y%m%d-%H%M%S)"
NAME="maxio-backup-${STAMP}"
if [[ -n "$LABEL" ]]; then
  NAME="${NAME}-${LABEL}"
fi

STAGING="${BACKUP_ROOT}/${NAME}"
ARCHIVE="${BACKUP_ROOT}/${NAME}.tar.gz"
CHECKSUM_FILE="${ARCHIVE}.sha256"

echo "==> P3-48 MaxIO backup"
echo "    source: ${DATA_DIR}"
echo "    dest:   ${ARCHIVE}"

rm -rf "$STAGING"
mkdir -p "$STAGING/data"

# Copy data tree preserving permissions where possible.
if command -v rsync >/dev/null 2>&1; then
  rsync -a "${DATA_DIR}/" "${STAGING}/data/"
else
  cp -a "${DATA_DIR}/." "${STAGING}/data/"
fi

cat > "${STAGING}/backup-manifest.txt" <<EOF
maxio_backup_format=1
created_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
source_data_dir=${DATA_DIR}
hostname=$(hostname 2>/dev/null || echo unknown)
includes_keyring=$([[ -f "${STAGING}/data/.maxio-keys.json" ]] && echo yes || echo no)
includes_credentials=$([[ -f "${STAGING}/data/.maxio-credentials.json" ]] && echo yes || echo no)
includes_metadata_index=$([[ -f "${STAGING}/data/.maxio-metadata.db" ]] && echo yes || echo no)
EOF

(
  cd "$STAGING"
  find . -type f ! -name SHA256SUMS | sort | while read -r rel; do
    rel="${rel#./}"
    sha256sum "$rel"
  done
) > "${STAGING}/SHA256SUMS"

tar -C "$BACKUP_ROOT" -czf "$ARCHIVE" "$NAME"
rm -rf "$STAGING"

(
  cd "$BACKUP_ROOT"
  sha256sum "$(basename "$ARCHIVE")" > "$(basename "$CHECKSUM_FILE")"
)

echo "==> Verifying archive checksum"
(
  cd "$BACKUP_ROOT"
  sha256sum -c "$(basename "$CHECKSUM_FILE")"
)

if [[ "$RETENTION_DAYS" =~ ^[0-9]+$ ]] && [[ "$RETENTION_DAYS" -gt 0 ]]; then
  echo "==> Pruning backups older than ${RETENTION_DAYS} days"
  find "$BACKUP_ROOT" -maxdepth 1 -type f -name 'maxio-backup-*.tar.gz' -mtime "+${RETENTION_DAYS}" -print -delete
  find "$BACKUP_ROOT" -maxdepth 1 -type f -name 'maxio-backup-*.tar.gz.sha256' -mtime "+${RETENTION_DAYS}" -print -delete
fi

echo "==> Backup complete"
echo "    ${ARCHIVE}"
echo "    ${CHECKSUM_FILE}"
echo ""
echo "Store a copy offline (removable media / vault). Loss of .maxio-keys.json with"
echo "SSE-S3 objects is permanent data loss — include keyring in every backup."