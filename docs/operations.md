# MaxIO Operations Guide

Production deployment checklist for MaxIO: networking, credentials, storage health, quotas, and backups.

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

MaxIO serves plain HTTP. Terminate TLS at your load balancer, ingress, or reverse proxy (nginx, Caddy, Traefik, etc.) and forward to MaxIO over HTTP on a trusted network.

Example nginx snippet:

```nginx
location / {
    proxy_pass http://127.0.0.1:9000;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    client_max_body_size 0;  # streaming uploads
}
```

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

Erasure coding is **server-wide** — there is no per-bucket toggle on a single instance. When enabled, new objects are stored as fixed-size chunks under `{key}.ec/` with a `manifest.json` sidecar and per-chunk SHA-256 checksums.

| Flag | Default | Description |
|------|---------|-------------|
| `MAXIO_ERASURE_CODING` / `--erasure-coding` | `false` | Enable chunked storage layout |
| `MAXIO_CHUNK_SIZE` / `--chunk-size` | `10485760` (10 MiB) | Data chunk size in bytes |
| `MAXIO_PARITY_SHARDS` / `--parity-shards` | `0` | Reed-Solomon parity shards per object (`0` = checksum-only, no recovery) |

**Operational limits**

- **Single-node only** — EC protects against bitrot and missing/corrupt chunks on one host; it is not replication or multi-node federation.
- **No per-bucket EC** — all new writes on an EC-enabled server use the chunked layout; existing flat objects remain readable until rewritten.
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

## Kubernetes

Minimal Deployment pattern:

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

## CI coverage

Pull requests run a `coverage` job that prints a `cargo llvm-cov` summary for library unit tests and enforces line-coverage floors via `scripts/check-coverage-floors.sh`:

| Module | Minimum line coverage |
|--------|----------------------|
| `src/storage/crypto.rs` | 80% |
| `src/auth/signature_v4.rs` | 25% |

Integration tests are excluded from these thresholds; they remain the primary S3 compatibility gate.

## Monitoring recommendations

- Alert on `/readyz` returning 503
- Monitor free disk space on the data volume; align alerts with `MAXIO_MIN_FREE_DISK_BYTES`
- Track 507 and `EntityTooLarge` rates as early signs of quota pressure
- Log shipping via container runtime or sidecar (MaxIO uses structured `tracing` on stderr)