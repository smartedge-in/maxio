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

Expose via Ingress with TLS. Console login rate limiting is in-memory; run a single replica for the console or accept per-pod limits until shared rate limiting is implemented (see backlog P1-06).

## Data backup

Back up the entire `MAXIO_DATA_DIR`, including:

- `buckets/` — object payload and metadata
- `.maxio-keys.json` — SSE-S3 keyring (unless you only use `MAXIO_MASTER_KEY` from a separate backup)
- `.maxio-readyz-probe` — transient probe file; safe to ignore

Restore by stopping MaxIO, restoring the directory tree, and starting again. Bucket and object layout is portable across hosts when permissions and paths are consistent.

## Default buckets

`MAXIO_DEFAULT_BUCKETS` creates buckets on startup (comma-separated). Invalid names are skipped. Provisioning is idempotent.

## Monitoring recommendations

- Alert on `/readyz` returning 503
- Monitor free disk space on the data volume; align alerts with `MAXIO_MIN_FREE_DISK_BYTES`
- Track 507 and `EntityTooLarge` rates as early signs of quota pressure
- Log shipping via container runtime or sidecar (MaxIO uses structured `tracing` on stderr)