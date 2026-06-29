# MaxIO Operations Guide

Production deployment checklist for MaxIO: networking, credentials, storage health, quotas, and backups.

**Deployment targets:** MaxIO must support **bare metal** (systemd + native binary) and **Kubernetes** (plain YAML manifests) as first-class production paths — see backlog P3-18, P1-24, epic P3-20, and `docs/plans/2026-06-29-deployment-targets.md`. Helm chart (P3-19) is a future improvement. Docker is supported as a packaging format for both.

## Bind address and exposure

MaxIO binds **`0.0.0.0` (all interfaces) by default** (`MAXIO_ADDRESS`). That exposes the S3 API and web console on every network interface on the host, which is convenient for containers but risky on machines with a public IP.

**Recommendations:**

- **Local development:** bind to loopback only.
- **Production:** run on a private network; expose only through a reverse proxy or Kubernetes ingress. Do not publish port 9000 directly to the internet.

```bash
MAXIO_ADDRESS=127.0.0.1 maxio --data-dir /data --port 9000
```

In Docker/Kubernetes, binding `0.0.0.0` inside the pod is normal when the Service/Ingress controls external access.

## TLS termination

MaxIO serves plain HTTP. Terminate TLS at your load balancer, ingress, or reverse proxy and forward to MaxIO over HTTP on a trusted network.

**Permissive-licensed proxies (recommended):** [Caddy](https://caddyserver.com/) (Apache-2.0), [Traefik](https://traefik.io/) (MIT), [Envoy](https://www.envoyproxy.io/) (Apache-2.0). On Kubernetes use Ingress or [MetalLB](https://metallb.io/) (Apache-2.0) — see `docs/plans/2026-06-29-permissive-ingress-ha.md` (P3-26).

**Not recommended** in MaxIO runbooks: **keepalived** and **HAProxy Community** (GPL-2.0), which conflict with the permissive-only policy in `docs/licensing.md`.

Example **Caddyfile** (reverse proxy to local MaxIO):

```caddyfile
:443 {
    tls internal   # replace with your ACME / cert paths in production
    reverse_proxy 127.0.0.1:9000 {
        flush_interval -1   # streaming uploads
    }
}
```

Set `MAXIO_TRUSTED_PROXIES` to your proxy CIDRs so rate limits and audit logs see real client IPs.

Set `MAXIO_SECURE_COOKIES=true` (default) when the console is served over HTTPS so session cookies include the `Secure` flag.

## Credentials

Set strong values before any production use:

```bash
export MAXIO_ACCESS_KEY="..."
export MAXIO_SECRET_KEY="..."
```

Do **not** use `--allow-insecure-dev` or `MAXIO_ALLOW_INSECURE_DEV=true` in production. That flag permits default credentials (`maxioadmin` / `maxioadmin`) and HTTP-only console cookies.

Rotate credentials by updating the environment and restarting MaxIO. Console session cookies include a **credential fingerprint** — sessions issued before rotation are rejected immediately after restart with new keys (users must log in again). Tokens still expire after 7 days when credentials are unchanged.

### Additional S3 access keys

Beyond the bootstrap pair, add keys in `<data-dir>/.maxio-credentials.json`:

```json
{
  "credentials": [
    {
      "access_key": "deploy-bot",
      "secret_key": "…",
      "enabled": true,
      "description": "CI uploads"
    }
  ]
}
```

Restrict file permissions (`chmod 600`). All enabled keys authenticate to the same global namespace (no per-key IAM scopes in v1). See `docs/plans/2026-06-28-multi-user-credentials.md`.

## Virtual-hosted-style URLs

Set the hostname clients use in virtual-hosted requests:

```bash
export MAXIO_SERVER_HOST="s3.example.com"
```

Behind TLS termination, this should match the public DNS name (port included for non-443 HTTP). Clients may then use `https://my-bucket.s3.example.com/key` in addition to path-style `https://s3.example.com/my-bucket/key`. Details: `docs/s3-compatibility.md`.

## Bucket policies

Upload a minimal public-read policy:

```bash
aws --endpoint-url http://localhost:9000 s3api put-bucket-policy --bucket photos --policy '{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": "*",
    "Action": "s3:GetObject",
    "Resource": "arn:aws:s3:::photos/*"
  }]
}'
```

v1 supports only Allow + `Principal:*` + `s3:GetObject` / `s3:ListBucket`. See `docs/plans/2026-06-28-bucket-policy-evaluation.md`.

## Testing S3 compatibility features

Unit tests cover virtual-host parsing, credential store loading, and policy evaluation (`cargo test -p maxio --lib`). Integration tests exercise virtual-hosted PUT/GET, secondary credentials, and bucket policy CRUD (`cargo test -p maxio --test integration virtual_host secondary bucket_policy`).

CI enforces **≥80% line coverage** on `api/virtual_host.rs`, `auth/credentials.rs`, and `storage/policy.rs` via `scripts/check-coverage-floors.sh`.

## SSE-S3 keyring backup

On first boot without `MAXIO_MASTER_KEY`, MaxIO creates `<data-dir>/.maxio-keys.json`. **Back up this file** with your data directory. Loss of all keyring keys makes SSE-S3 encrypted objects unrecoverable.

To supply a fixed master key:

```bash
export MAXIO_MASTER_KEY="$(openssl rand -base64 32)"
```

Store the value in your secrets manager. The on-disk keyring file is still merged for decrypting objects written under older keys.

## Trusted reverse proxies

By default, MaxIO **does not trust** `X-Forwarded-For` — client IP for console login and rate limits comes from the direct TCP peer. Behind a load balancer, configure known proxy CIDRs:

```bash
export MAXIO_TRUSTED_PROXIES="10.0.0.0/8,192.168.1.0/24"
```

When the direct peer matches a trusted CIDR, MaxIO walks `X-Forwarded-For` from the right, stripping trusted hops, and uses the leftmost remaining address for login rate limiting and S3/admin per-IP limits. Only list proxies you control; never trust the public internet.

## Console login rate limiting (multi-replica)

The default login rate limiter is **in-memory** (10 attempts per 5 minutes per IP). It applies per MaxIO process — multiple console replicas without a shared store each maintain independent counters.

For horizontally scaled deployments, set a Redis backend:

```bash
export MAXIO_LOGIN_RATE_LIMIT_REDIS_URL="redis://redis.internal:6379"
```

When unset, run a single console replica or accept per-replica limits.

## Health probes

| Endpoint | Purpose | Success | Failure |
|----------|---------|---------|---------|
| `/healthz` | Liveness — process is running | `200` | n/a |
| `/healthz?verbose=1` | Liveness + subsystem JSON (disk, uploads, housekeeping) | `200` | n/a |
| `/readyz` | Readiness — storage is usable | `200` | `503` |

`/healthz?verbose=1` returns JSON including uptime, `readyz` status, disk free percent, active multipart upload count, and seconds since the last housekeeping sweep. Use it for deeper monitoring without hitting the admin API.

`/readyz` checks that the data directory exists, is writable (write probe), and the SSE-S3 keyring has at least one key. Use `/healthz` for liveness and `/readyz` for readiness in Kubernetes:

```yaml
livenessProbe:
  httpGet:
    path: /healthz
    port: 9000
  periodSeconds: 10
readinessProbe:
  httpGet:
    path: /readyz
    port: 9000
  periodSeconds: 5
```

The built-in `maxio healthcheck` subcommand defaults to `/healthz`. Point it at `/readyz` when you need storage-aware checks:

```bash
maxio healthcheck --url http://127.0.0.1:9000/readyz
```

## Admin API and `maxio-admin` CLI

MaxIO exposes a versioned admin HTTP API at `/api/admin/v1/*` for remote operations (status, disk usage, doctor checks, housekeeping, keyring metadata). **Use TLS in production** — terminate HTTPS at your reverse proxy and restrict network access to the admin paths.

### Authentication

| Method | When to use |
|--------|-------------|
| `Authorization: Bearer <token>` | Preferred — set `MAXIO_ADMIN_TOKEN` on the server and `admin_token` in the CLI profile |
| `Authorization: Basic <base64(access:secret)>` | Fallback — same credentials as S3 (`MAXIO_ACCESS_KEY` / `MAXIO_SECRET_KEY`) |

Requests without valid credentials receive HTTP `401`. The API is rate-limited per client IP (`MAXIO_ADMIN_RATE_MAX`, default 120 requests per 60 seconds).

Server configuration:

```bash
export MAXIO_ADMIN_TOKEN="$(openssl rand -hex 32)"
export MAXIO_ADMIN_RATE_MAX=120
export MAXIO_ADMIN_RATE_WINDOW_SECS=60
```

### Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/admin/v1/status` | Liveness + readiness summary, version, uptime |
| `GET` | `/api/admin/v1/info` | Data directory, disk usage, bucket/object counts, active config |
| `GET` | `/api/admin/v1/doctor` | Readiness, disk reserve, keyring checks |
| `GET` | `/api/admin/v1/buckets` | Bucket list with object counts |
| `GET` | `/api/admin/v1/buckets/{name}` | Single bucket metadata |
| `GET` | `/api/admin/v1/keyring` | Key ids and metadata (never raw key material) |
| `POST` | `/api/admin/v1/housekeeping/run` | On-demand stale-multipart and temp-file sweep |

Example (behind nginx TLS termination):

```bash
curl -sS -H "Authorization: Bearer $MAXIO_ADMIN_TOKEN" \
  https://maxio.example.com/api/admin/v1/status | jq .
```

### `maxio-admin` CLI

`maxio-admin` is a **separate workspace crate** (`crates/maxio-admin`) and operator client — it is not deployed as a cluster replica. Target boundary (P3-17): depend on `maxio-storage` for local `--data-dir` commands only; remote commands use the admin HTTP API with no link to `maxio-server`. See `docs/plans/2026-06-29-admin-cli-crate.md`.

Build from the repository root:

```bash
cargo build -p maxio-admin
cp crates/maxio-admin/config.example.toml ~/.config/maxio/config.toml
# Edit admin_token and endpoint for your profile
```

| Command | Scope | Description |
|---------|-------|-------------|
| `maxio-admin status` | remote | Health + readiness |
| `maxio-admin info` | remote | Disk, counts, server config |
| `maxio-admin doctor` | remote | Preflight checks |
| `maxio-admin doctor --data-dir /data` | **local** | Offline doctor (no network) |
| `maxio-admin buckets list` | remote | Bucket inventory |
| `maxio-admin buckets head <name>` | remote | Single bucket |
| `maxio-admin housekeeping run` | remote | Trigger maintenance sweep |
| `maxio-admin keyring list` | remote | Keyring metadata |
| `maxio-admin keyring rotate --data-dir /data` | **local** | Rotate on-disk keyring |

Global flags: `--profile`, `--endpoint`, `--json`, `--config` (default `~/.config/maxio/config.toml`). Environment overrides: `MAXIO_ADMIN_PROFILE`, `MAXIO_ADMIN_ENDPOINT`, `MAXIO_ADMIN_CONFIG`.

```bash
export MAXIO_ADMIN_ENDPOINT=https://maxio.example.com
maxio-admin --profile prod --json doctor
```

## Upload quotas and disk reserve

Optional limits protect against oversized uploads and disk exhaustion:

| Variable | Default | Description |
|----------|---------|-------------|
| `MAXIO_MAX_OBJECT_BYTES` | `0` (unlimited) | Reject uploads larger than this with S3 `EntityTooLarge` |
| `MAXIO_MIN_FREE_DISK_BYTES` | `0` (disabled) | Reject new uploads when free space on the data volume falls below this reserve (HTTP 507) |

Example — cap objects at 5 GiB and keep 10 GiB free:

```bash
export MAXIO_MAX_OBJECT_BYTES=5368709120
export MAXIO_MIN_FREE_DISK_BYTES=10737418240
```

When `Content-Length` is present, the limit is enforced before streaming begins. Without `Content-Length`, enforcement happens as bytes are read.

## Erasure coding (single-node)

Erasure coding is controlled **server-wide** via `MAXIO_ERASURE_CODING`. When enabled, buckets may override layout per bucket with `PUT ?erasure` (stored in `.bucket.json` as `erasure_coding`). New writes use the effective policy; existing flat objects stay flat until rewritten.

| Flag | Default | Description |
|------|---------|-------------|
| `MAXIO_ERASURE_CODING` / `--erasure-coding` | `false` | Enable chunked storage layout |
| `MAXIO_CHUNK_SIZE` / `--chunk-size` | `10485760` (10 MiB) | Data chunk size in bytes |
| `MAXIO_PARITY_SHARDS` / `--parity-shards` | `0` | Reed-Solomon parity shards per object (`0` = checksum-only, no recovery) |

**Operational limits**

- **Single-node only** — EC protects against bitrot and missing/corrupt chunks on one host; it is not replication or multi-node federation.
- **Per-bucket EC** — with server EC on, set `ErasureConfiguration` to `Disabled` on a bucket to keep flat layout; unset/`Enabled` uses chunked writes. Chunk size and parity remain server-wide.
- **Parity required for recovery** — without `--parity-shards`, corrupt or missing chunks fail reads with S3 `InternalError` (HTTP 500). With parity, MaxIO attempts Reed-Solomon reconstruction when a data chunk fails its checksum.
- **GF(2⁸) shard cap** — `data_chunks + parity_shards` must not exceed **255**. If an object would exceed this, increase `--chunk-size` (fewer data chunks) or reduce parity.
- **Read errors** — chunk verification and unrecoverable RS failures return structured S3 XML (`InternalError`) before the response body streams, rather than dropping the connection mid-read.

Example — checksum-only EC (detect corruption, no recovery):

```bash
maxio --data-dir /data --erasure-coding --chunk-size 10485760
```

Example — EC with two parity shards per object (recover up to two missing/corrupt data chunks per stripe):

```bash
maxio --data-dir /data --erasure-coding --chunk-size 1048576 --parity-shards 2
```

The main-branch `aws-cli` CI job runs `tests/aws_cli_test.sh` against a server with `--erasure-coding` so on-disk corruption checks are not skipped.

## Metadata index (optional)

Enable with `MAXIO_METADATA_INDEX=true` to maintain `{data_dir}/.maxio-metadata.db` (SQLite WAL). The index accelerates `ListObjects` on large buckets; the filesystem remains authoritative.

| Flag | Default | Description |
|------|---------|-------------|
| `MAXIO_METADATA_INDEX` / `--metadata-index` | `false` | Maintain SQLite object index |

On startup with the index enabled, MaxIO rebuilds per-bucket rows from a full filesystem walk. Writes and deletes upsert/remove index rows; if the index is disabled, listing falls back to directory walk.

## Lifecycle expiration

Prefix-based expiration rules are stored on `BucketMeta.lifecycle_rules` and enforced during the hourly housekeeping sweep (non-versioned buckets only).

| API | Description |
|-----|-------------|
| `PUT /{bucket}?lifecycle` | Install rules (XML subset: `ID`, `Prefix`, `Status`, `Expiration/Days`) |
| `GET /{bucket}?lifecycle` | Read configuration |
| `DELETE /{bucket}?lifecycle` | Remove configuration |

Versioned buckets skip automatic expiration in v1.

## Storage backend abstraction (P1-15)

Single-node deployments use `FilesystemStorage` behind the `StorageBackend` trait (`crates/maxio-storage/src/backend.rs`). The server tier holds a `DynStorage` handle (`Arc<dyn StorageBackend>`) so metadata and object mutations can later route through a Raft-backed implementation without changing HTTP handlers.

| Component | Location |
|-----------|----------|
| Trait + `dyn_storage()` | `crates/maxio-storage/src/backend.rs` |
| Server wiring | `AppState.storage: DynStorage` in `maxio-server` |
| Raft apply path | P1-17 — `maxio-cluster` storage Raft; mutations ordered on leader, applied locally |

Today all I/O is local filesystem. Integration tests exercise the trait boundary unchanged.

## Replication / federation

Not implemented. **Priority 1** path is **Raft-first multi-replica** (not operator sync):

| Backlog | Scope |
|---------|-------|
| ~~P1-14~~ ✓ | Live multi-node: dual Raft + distributed EC — `docs/plans/2026-06-29-multi-replica-raft-priority.md` |
| ~~P1-15~~ ✓ | `StorageBackend` trait — prerequisite for Raft apply |
| ~~P1-16~~ ✓ | OpenRaft `0.9` spike + license gate — `docs/plans/2026-06-29-raft-library-spike.md` |
| ~~P1-22~~ ✓ | `maxio-common` — shared `VERSION`, admin DTOs, routing snapshots |
| ~~P1-17~~ ✓ | Storage tier Raft — `docs/plans/2026-06-29-storage-raft-implementation.md` |
| ~~P1-18–P1-21~~ ✓ | Distributed EC → Server routing → stateless `maxio-ui` |
| ~~P1-24~~ ✓ | `scripts/cluster-test.sh`; `deploy/k8s/distributed/` |
| ~~P3-09–P3-11~~ | Operator `rsync`/agent track — **deferred** |

Erasure coding supports single-node (default) and distributed shard placement (`maxio-cluster`, P1-18/P1-19).

### Asymmetric scale-out (P1-14)

Distributed layouts use separate replica counts per tier (`deploy/k8s/distributed/`). Single-node colocated mode remains the default (`MAXIO_CLUSTER_MODE=false`, embedded UI).

| Tier | Consensus | Status |
|------|-----------|--------|
| `maxio-ui` | None (stateless static SPA) | Shipped (`crates/maxio-ui`) |
| `maxio-server` | Routing snapshot (`ClusterState`) | Shipped (`MAXIO_CLUSTER_MODE`) |
| `maxio-storage` | Storage Raft quorum (`maxio-cluster`) | Shipped (in-process; HTTP join TBD) |
| Epic (asymmetric replicas) | All three tiers | P1-14 closed |

Storage and server each elect their own Raft leader. UI replicas are interchangeable and hold no session state. Replica counts may differ (e.g. 3 UI, 3 server, 5 storage). See `docs/plans/2026-06-29-ui-scale-out.md`.

## Docker

```bash
docker run -d \
  --name maxio \
  -p 9000:9000 \
  -v maxio-data:/data \
  -e MAXIO_ACCESS_KEY=... \
  -e MAXIO_SECRET_KEY=... \
  -e MAXIO_MAX_OBJECT_BYTES=5368709120 \
  ghcr.io/coollabsio/maxio
```

Mount the data volume on durable storage (SSD, network block volume). Bind-mounting an NFS path works but latency affects listing performance.

## Bare metal (planned, P3-18)

Target layout: release binary, dedicated `maxio` user, `MAXIO_DATA_DIR` on local SSD, systemd unit, TLS at host reverse proxy. Multi-host tier separation (storage / server / UI) aligns with P3-13. Full runbook: `docs/plans/2026-06-29-deployment-targets.md`.

## Kubernetes

Official manifests live under `deploy/k8s/` — single-node (`single-node/`) and distributed tiers (`distributed/`). Helm chart is optional later (P3-19).

**Distributed cluster (P1-14):**

```bash
kubectl apply -f deploy/k8s/distributed/
# Replace REGISTRY/maxio:VERSION and REGISTRY/maxio-ui:VERSION in manifests before apply.
```

**Server tier flags:** `MAXIO_SERVE_UI=false` when using standalone `maxio-ui`; `MAXIO_CLUSTER_MODE=true` so `/readyz` requires storage quorum in the routing snapshot.

**Local harness:** `bash scripts/cluster-test.sh` runs in-process 3-node Raft acceptance tests.

Single-node minimal pattern (`replicas: 1`):

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: maxio
spec:
  replicas: 1
  selector:
    matchLabels:
      app: maxio
  template:
    metadata:
      labels:
        app: maxio
    spec:
      containers:
        - name: maxio
          image: ghcr.io/coollabsio/maxio:latest
          ports:
            - containerPort: 9000
          env:
            - name: MAXIO_DATA_DIR
              value: /data
            - name: MAXIO_ACCESS_KEY
              valueFrom:
                secretKeyRef:
                  name: maxio-credentials
                  key: access-key
            - name: MAXIO_SECRET_KEY
              valueFrom:
                secretKeyRef:
                  name: maxio-credentials
                  key: secret-key
          volumeMounts:
            - name: data
              mountPath: /data
          livenessProbe:
            httpGet:
              path: /healthz
              port: 9000
          readinessProbe:
            httpGet:
              path: /readyz
              port: 9000
      volumes:
        - name: data
          persistentVolumeClaim:
            claimName: maxio-data
```

Expose via Ingress with TLS.

## Rate limiting

MaxIO applies in-memory per-client-IP rate limits (sliding window). Limits are per process; behind multiple replicas each pod enforces its own counters until shared rate limiting lands (P1-06).

| Limit | Env vars | Default | Behavior |
|-------|----------|---------|----------|
| Console login | _(fixed)_ | 10 attempts / 5 min | JSON `429` on `/api/auth/login` |
| S3 auth failures | `MAXIO_S3_RATE_AUTH_MAX`, `MAXIO_S3_RATE_AUTH_WINDOW_SECS` | 60 / 300 s | S3 XML `SlowDown`, HTTP `429`, `Retry-After` |
| S3 PUT uploads | `MAXIO_S3_RATE_PUT_MAX`, `MAXIO_S3_RATE_PUT_WINDOW_SECS` | disabled | Same `SlowDown` response |

Set `MAXIO_S3_RATE_AUTH_MAX=0` or `MAXIO_S3_RATE_PUT_MAX=0` to disable a limit. Tune PUT limits when exposing MaxIO directly to untrusted networks.

Console login rate limiting is in-memory; run a single replica for the console or accept per-pod limits until shared rate limiting is implemented (see backlog P1-06).

## Content Security Policy

The console is served at `/ui/` with a strict CSP on all routes:

- **Scripts:** `script-src 'self'` only. The theme bootstrap runs from `/ui/theme-init.js` (no inline scripts).
- **Styles:** `style-src 'self' 'unsafe-inline'` — Svelte injects component-scoped styles inline; removing this would break the UI until a build-time hash/nonce pipeline exists.

Review `CONTENT_SECURITY_POLICY` in `src/server.rs` when changing the frontend build.

## Data backup

Back up the entire `MAXIO_DATA_DIR`, including:

- `buckets/` — object payload and metadata
- `.maxio-keys.json` — SSE-S3 keyring (unless you only use `MAXIO_MASTER_KEY` from a separate backup)
- `.maxio-readyz-probe` — transient probe file; safe to ignore

Restore by stopping MaxIO, restoring the directory tree, and starting again. Bucket and object layout is portable across hosts when permissions and paths are consistent.

## Default buckets

`MAXIO_DEFAULT_BUCKETS` creates buckets on startup (comma-separated). Invalid names are skipped. Provisioning is idempotent.

## Local CI and validation (`make ci`)

The repository root **Makefile** runs an extended validation pipeline beyond GitHub Actions.
Use it before release builds or when validating security and licensing locally.

### Versioning

Release versions follow [Semantic Versioning](https://semver.org/). The canonical version is in
the repository root **`VERSION`** file. After bumping it, run `make sync-version` (or any target
that depends on it, such as `make release`). Container images built via `make image` are tagged
`maxio:v<VERSION>` by default.

### One-time setup

```bash
# Run as your normal user — do not use sudo (rustup/cargo install per-user)
make install-tools
```

This installs Rust components (`rustfmt`, `clippy`, `llvm-tools-preview`), `cargo-audit`,
`cargo-deny`, `cargo-llvm-cov`, bun (when `unzip` is available), and Trivy to `~/.local/bin`.

### Full pipeline

```bash
make ci          # same as make all — stops on first failure
```

Stages (in order): `fmt` → `check` → `lint` → `test` → `coverage` → `audit` → `deny` →
Trivy filesystem/secret/config/license scans → SBOM → `trivy-sbom` → `doc` → `cargo clean` →
`release` → `image` → `trivy-image`.

**Partial runs** — invoke individual targets, e.g. `make test`, `make deny-all`, `make trivy-fs-critical`.

**Without bun** — when bun is not on `PATH`, `SKIP_FRONTEND=1` is set automatically and the
embedded UI build is skipped (minimal stub UI in release binaries). Install bun via
`make install-tools` for a full console build.

**Without Docker** — `image` and `trivy-image` fail if Docker is not installed; earlier stages
still run. Skip container steps with:

```bash
make fmt check lint test coverage audit deny trivy-fs release
```

### Disk space

A full `make ci` on a single 20 GiB root volume needs roughly **10+ GiB** free at peak:
debug `target/` from test/coverage/doc (~5–6 GiB), Trivy DB (~100 MiB), then release compile.

The Makefile mitigates exhaustion on small disks:

- Trivy cache defaults to `/tmp/maxio-trivy-cache` (tmpfs), not the repo tree
- `cargo clean` runs after `doc` and before `release` to drop debug artifacts

If a release build fails with `No space left on device`:

```bash
cargo clean
rm -rf /tmp/maxio-trivy-cache
make release
```

### Security scan notes

Trivy may report **MEDIUM** Dockerfile hints (`DS-0013`: prefer `WORKDIR` over `RUN cd …`).
These do not fail `make ci` by default. See `docs/licensing.md` for the `cargo audit` /
`cargo-deny` advisory policy.

## CI coverage

Pull requests run a `coverage` job that prints a `cargo llvm-cov` summary for library unit tests and enforces line-coverage floors via `scripts/check-coverage-floors.sh`:

| Module | Minimum line coverage |
|--------|----------------------|
| `crates/maxio-storage/src/crypto.rs` | 80% |
| `crates/maxio-server/src/auth/signature_v4.rs` | 25% |
| `crates/maxio-server/src/api/virtual_host.rs` | 80% |
| `crates/maxio-server/src/auth/credentials.rs` | 80% |
| `crates/maxio-storage/src/policy.rs` | 80% |

Integration tests are excluded from these thresholds; they remain the primary S3 compatibility gate.

## Prometheus metrics

Enable with `MAXIO_METRICS_ENABLED=true` (or `--metrics-enabled`). Scrape `GET /metrics` on the main HTTP listener.

| Variable | Default | Description |
|----------|---------|-------------|
| `MAXIO_METRICS_ENABLED` | `false` | Register `/metrics` on the main port |
| `MAXIO_METRICS_PORT` | `0` | Optional dedicated metrics-only listener (same bind address) |

Exported series include `maxio_http_requests_total{method,status_class}`, request duration sum/count, `maxio_s3_slow_down_total`, `maxio_upload_bytes_total`, `maxio_uptime_seconds`, `maxio_disk_free_bytes`, `maxio_disk_total_bytes`, and `maxio_active_multipart_uploads`.

Example:

```bash
export MAXIO_METRICS_ENABLED=true
curl -sS http://127.0.0.1:9000/metrics | head
```

## Structured audit log

Enable with `MAXIO_AUDIT_LOG=true` (or `--audit-log`). MaxIO emits one JSON object per line on the `maxio_audit` tracing target for mutating requests (`PUT`, `POST`, `DELETE`, `PATCH`) on S3, console (`/api/`), and admin (`/api/admin/v1/`) routes.

Fields: `timestamp`, `source` (`s3` | `console` | `admin`), `action`, `method`, `path`, `bucket`, `key`, `principal` (SigV4 access key for S3; `admin:bearer` or Basic access key for admin API; `console` for authenticated console routes), `client_ip`, `status`, `outcome` (`success` | `failure`).

Audit middleware runs after route authentication so S3 requests include the verified access key in `principal`.

Pipe stderr through your log agent or set a JSON filter on `target=maxio_audit`:

```bash
export MAXIO_AUDIT_LOG=true
export RUST_LOG=info
maxio serve --data-dir /data 2>&1 | grep maxio_audit
```

## Monitoring recommendations

- Alert on `/readyz` returning 503
- Scrape `/metrics` when `MAXIO_METRICS_ENABLED` is set (request rate, 5xx ratio, disk free bytes)
- Monitor free disk space on the data volume; align alerts with `MAXIO_MIN_FREE_DISK_BYTES`
- Track 507 and `EntityTooLarge` rates as early signs of quota pressure
- Log shipping via container runtime or sidecar (MaxIO uses structured `tracing` on stderr)