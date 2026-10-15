#!/usr/bin/env bash
# Caller: CI release-credentials.yml, release-linux-arm64.yml; skill github-release
# The Mac's native arm64 Docker builder image exists and carries the sccache the release build wraps rustc with.
set -euo pipefail
IMAGE="${LINUX_ARM64_BUILDER_IMAGE:-codeterm-build-linux-arm64}"
docker info >/dev/null || { echo "::error::Docker is not running on the Linux arm64 builder" >&2; exit 1; }
docker image inspect "$IMAGE" >/dev/null || { echo "::error::$IMAGE is missing; run bake-linux-images.yml" >&2; exit 1; }
arch="$(docker image inspect --format '{{.Architecture}}' "$IMAGE")"
[ "$arch" = arm64 ] || { echo "::error::$IMAGE is $arch, not arm64" >&2; exit 1; }
docker run --rm --pull=never --platform linux/arm64 "$IMAGE" sccache --version \
  || { echo "::error::$IMAGE has no working sccache; rebake it from .codeterm/build-machine/docker" >&2; exit 1; }
echo "$IMAGE: arm64 with sccache"
