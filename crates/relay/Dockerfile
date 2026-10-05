# syntax=docker/dockerfile:1
# Build from the repository root:
#   docker build -f crates/relay/Dockerfile -t spool-relay .
# Cross-compiles on the build host for each target platform (no emulation).
FROM --platform=$BUILDPLATFORM rust:1-slim-bookworm AS build
ARG TARGETARCH
WORKDIR /src
RUN case "$TARGETARCH" in \
      amd64) echo x86_64-unknown-linux-musl ;; \
      arm64) echo aarch64-unknown-linux-musl ;; \
      *) echo "unsupported arch $TARGETARCH" >&2; exit 1 ;; \
    esac > /rust-target \
 && rustup target add "$(cat /rust-target)"
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
COPY tools tools
ENV RUSTFLAGS="-C linker=rust-lld -C target-feature=+crt-static"
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p spool-relay --target "$(cat /rust-target)" \
 && cp "target/$(cat /rust-target)/release/spool-relay" /spool-relay

FROM scratch
COPY --from=build /spool-relay /spool-relay
USER 65534:65534
ENV SPOOL_LISTEN=0.0.0.0:8080
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=3s --retries=3 CMD ["/spool-relay", "healthcheck"]
ENTRYPOINT ["/spool-relay"]
