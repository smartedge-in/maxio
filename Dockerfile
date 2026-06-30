FROM rust:1-bookworm AS builder

ARG MAXIO_VERSION=0.0.0

RUN curl -fsSL https://bun.sh/install | bash
ENV PATH="/root/.bun/bin:${PATH}"

WORKDIR /app

COPY ui/package.json ui/bun.lock ./ui/
RUN cd ui && bun install --frozen-lockfile

COPY VERSION scripts/sync-version.sh Cargo.toml Cargo.lock build.rs ./
COPY crates ./crates
COPY src ./src
COPY tests ./tests
COPY ui ./ui

RUN chmod +x scripts/sync-version.sh && ./scripts/sync-version.sh
RUN cd ui && bun run build
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

ARG MAXIO_VERSION=0.0.0
LABEL org.opencontainers.image.version="${MAXIO_VERSION}"
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && groupadd --system --gid 10001 maxio \
  && useradd --system --uid 10001 --gid 10001 --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin maxio \
  && mkdir -p /data \
  && chown -R maxio:maxio /data

COPY --from=builder /app/target/release/maxio /usr/local/bin/maxio

ENV MAXIO_DATA_DIR="/data"
EXPOSE 9000
VOLUME ["/data"]
USER maxio:maxio
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD ["maxio", "healthcheck", "--url", "http://127.0.0.1:9000/healthz", "--timeout-ms", "2000"]

ENTRYPOINT ["maxio"]
CMD ["serve"]
