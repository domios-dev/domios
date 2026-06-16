# Windows compile-check image: cargo-xwin cross-compiles to
# x86_64-pc-windows-msvc from Linux. Catches #[cfg(windows)] breakage
# locally; real Windows test execution happens on a Windows mesh peer.
# Based on ubuntu:24.04 (shared with test-linux.dockerfile) instead of
# rust:bookworm — the base layer is already cached locally.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential pkg-config \
    libssl-dev \
    clang lld llvm \
 && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal \
      --target x86_64-pc-windows-msvc
ENV PATH="/root/.cargo/bin:${PATH}"

RUN cargo install --locked cargo-xwin

WORKDIR /work
