# Multi-user credentials (P1-10)

## Goal

Support more than one S3 access/secret key pair without a full IAM service.

## Phase 1 (implemented)

- **Bootstrap credential** — `MAXIO_ACCESS_KEY` / `MAXIO_SECRET_KEY` (unchanged).
- **Additional credentials** — optional file `<data-dir>/.maxio-credentials.json`:

```json
{
  "credentials": [
    {
      "access_key": "app-user",
      "secret_key": "…",
      "enabled": true,
      "description": "CI deploy key"
    }
  ]
}
```

- **Lookup** — S3 Signature V4, console login, and admin API Basic auth all resolve secrets via `CredentialStore`.
- **Disabled entries** — `"enabled": false` excludes a key from authentication.
- **Rotation** — change bootstrap env vars or edit the JSON file and restart; console sessions still invalidate via credential fingerprint (P1-05).

## Operational notes

- Protect `.maxio-credentials.json` like the data directory (mode `600`, backups encrypted).
- Duplicate `access_key` values in the file override earlier entries; bootstrap env key is always present on startup.
- Startup logs the count when more than one enabled key is loaded.

## Non-goals (future phases)

| Item | Reason deferred |
|------|-----------------|
| IAM users, groups, roles | Requires identity store and policy binding per principal |
| Per-key bucket/policy scopes | Needs policy engine v2+ and authorization layer |
| STS / temporary session tokens | Separate token service and signing path |
| External IdP (OIDC, LDAP) | Federation gateway or sidecar |
| Hot reload without restart | File watcher + atomic credential swap |
| Admin API to manage keys | Use config management / GitOps for v1 |

## Verification

- Unit: `cargo test -p maxio --lib credentials`
- Integration: `cargo test -p maxio --test integration secondary`
- Coverage floor: ≥80% lines on `auth/credentials.rs` (see `scripts/check-coverage-floors.sh`)

## Phase 2 sketch

1. Admin API CRUD for credentials (encrypted at rest).
2. Optional per-key labels and `maxio-admin keys list`.
3. Scoped keys (read-only, bucket prefix) once policy engine supports principals.