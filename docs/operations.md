# MaxIO Operations Guide

Production deployment checklist for MaxIO: networking, credentials, storage health, quotas, and backups.

## Bind address and exposure

MaxIO binds `0.0.0.0` by default (`MAXIO_ADDRESS`). For local development, prefer `127.0.0.1`. In production, run MaxIO on a private network and expose it only through a reverse proxy or ingress controller.

```bash
MAXIO_ADDRESS=127.0.0.1 maxio --data-dir /data --port 9000
```

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

Rotate credentials by updating the environment and restarting MaxIO. Console sessions issued before rotation remain valid until they expire (7 days); plan a maintenance window or force users to re-login.

## SSE-S3 keyring backup

On first boot without `MAXIO_MASTER_KEY`, MaxIO creates `<data-dir>/.maxio-keys.json`. **Back up this file** with your data directory. Loss of all keyring keys makes SSE-S3 encrypted objects unrecoverable.

To supply a fixed master key:

```bash
export MAXIO_MASTER_KEY="$(openssl rand -base64 32)"
```

Store the value in your secrets manager. The on-disk keyring file is still merged for decrypting objects written under older keys.

## Health probes

| Endpoint | Purpose | Success | Failure |
|----------|---------|---------|---------|
| `/healthz` | Liveness — process is running | `200` | n/a |
| `/readyz` | Readiness — storage is usable | `200` | `503` |

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