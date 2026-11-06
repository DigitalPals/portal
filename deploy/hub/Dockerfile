# Build from the repository root: docker build -f deploy/hub/Dockerfile .
FROM rust:1.93.1-trixie AS build
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
# Cargo resolves all workspace members, but builds only the headless Hub. Keep
# local targets, .git, credentials and unrelated host files out of this stage.
COPY src ./src
COPY vendor ./vendor
COPY hub ./hub
RUN cargo build --locked --release -p portal-hub

FROM debian:trixie-slim
ARG HUB_VERSION=0.5.0-beta.17
LABEL org.opencontainers.image.title="Portal Hub" \
      org.opencontainers.image.source="https://github.com/digitalpals/portal" \
      org.opencontainers.image.version="${HUB_VERSION}" \
      io.portal.hub.api-version="2" \
      io.portal.hub.deployment-protocol="1"
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl openssh-client dtach tmux bsdutils libssl3t64 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 portal-hub \
    && useradd --uid 10001 --gid 10001 --home-dir /var/lib/portal-hub --no-create-home --shell /usr/sbin/nologin portal-hub \
    && install -d -o 10001 -g 10001 -m 0700 /var/lib/portal-hub /run/portal-hub
COPY --from=build /build/target/release/portal-hub /usr/local/bin/portal-hub
USER 10001:10001
ENV PORTAL_HUB_STATE_DIR=/var/lib/portal-hub \
    PORTAL_HUB_RUN_DIR=/run/portal-hub \
    PORTAL_HUB_MAX_LOG_BYTES=16777216 \
    HOME=/var/lib/portal-hub
ENTRYPOINT ["/usr/local/bin/portal-hub"]
CMD ["web", "--bind", "127.0.0.1:8080"]
