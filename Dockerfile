FROM rust:1-bookworm AS builder

RUN apt-get update && \
    apt-get install -y musl-tools gcc-aarch64-linux-gnu && \
    rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

ARG TARGETARCH
RUN case "${TARGETARCH}" in \
      amd64) \
        TARGET=x86_64-unknown-linux-musl ;; \
      arm64) \
        TARGET=aarch64-unknown-linux-musl && \
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc \
               CC_aarch64_unknown_linux_musl=aarch64-linux-gnu-gcc ;; \
      *)     echo "unsupported arch: ${TARGETARCH}" && exit 1 ;; \
    esac && \
    cargo build --release --target "${TARGET}" && \
    cp "target/${TARGET}/release/lox" /lox

FROM scratch
COPY --from=builder /lox /lox
ENTRYPOINT ["/lox"]
