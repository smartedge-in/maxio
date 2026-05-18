FROM rust:1-bookworm AS builder

RUN curl -fsSL https://bun.sh/install | bash
ENV PATH="/root/.bun/bin:${PATH}"

WORKDIR /app

COPY ui/package.json ui/bun.lock ./ui/
RUN cd ui && bun install --frozen-lockfile

COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY tests ./tests
COPY ui ./ui

RUN cd ui && bun run build
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --create-home --home-dir /nonexistent --shell /usr/sbin/nologin maxio \
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
