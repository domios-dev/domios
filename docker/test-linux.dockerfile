# Unit-test image for the Linux lane of scripts/test-matrix.sh.
# ubuntu:24.04 is required: pyke/ort prebuilt onnxruntime needs glibc >= 2.38.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential pkg-config \
    libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
    librsvg2-dev libssl-dev patchelf \
 && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"

RUN npm install -g pnpm@10.32.1

WORKDIR /work
