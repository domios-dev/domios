#!/usr/bin/env bash
# Windows EXECUTION lane: cross-compile test binaries with cargo-xwin in
# Docker, then run them on a real Windows ARM64 VM (QEMU, see
# ~/VMs/codeterm-win11-arm/vm.sh) over SSH. Slower than windows-cross but
# actually executes the tests — ConPTY and all.
#
# Requires: VM running (vm.sh run), sshpass or key auth for test@localhost:2222.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
IMAGE=codeterm-test-windows-cross
SSH_PORT="${CODETERM_WINVM_PORT:-2222}"
SSH_DEST="${CODETERM_WINVM_DEST:-test@127.0.0.1}"
SSH_PASS="${CODETERM_WINVM_PASS:-test}"
SSH_OPTS=(-p "$SSH_PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10)

SSH() { sshpass -p "$SSH_PASS" ssh "${SSH_OPTS[@]}" "$SSH_DEST" "$@"; }
SCP() { sshpass -p "$SSH_PASS" scp -P "$SSH_PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null "$@"; }

SSH "echo VM reachable" \
  || { echo "Windows VM not reachable on $SSH_DEST:$SSH_PORT — start it: ~/VMs/codeterm-win11-arm/vm.sh run"; exit 7; }

docker build -q -t "$IMAGE" -f "$ROOT/docker/test-windows-cross.dockerfile" "$ROOT/docker" >/dev/null

echo "=== building Windows test binaries (cargo xwin test --no-run) ==="
BINS=$(docker run --rm \
  -v "$ROOT:/work" \
  -v codeterm-xwin-cargo-registry:/root/.cargo/registry \
  -v codeterm-xwin-cache:/root/.cache/cargo-xwin \
  -v codeterm-xwin-target:/work/src-tauri/target \
  -v codeterm-xwin-cli-target:/work/cli/target \
  -e XWIN_ACCEPT_LICENSE=1 \
  -w /work \
  "$IMAGE" bash -c '
    set -euo pipefail
    mkdir -p dist src-tauri/bundled-embed src-tauri/bundled-cli
    touch src-tauri/bundled-embed/codeterm-embed-x86_64-pc-windows-msvc.exe
    [ -e src-tauri/bundled-cli/codeterm ] || touch src-tauri/bundled-cli/codeterm
    cargo xwin test --locked -j 4 --target x86_64-pc-windows-msvc \
      --manifest-path src-tauri/Cargo.toml --lib --no-run \
      --message-format=json 2>/dev/null \
      | grep -o "\"executable\":\"[^\"]*\.exe\"" | cut -d\" -f4
  ')
[ -n "$BINS" ] || { echo "no test executables produced"; exit 1; }

# Copy binaries out of the docker volume to a host temp dir for scp.
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
for bin in $BINS; do
  docker run --rm -v codeterm-xwin-target:/t -v "$STAGE:/out" "$IMAGE" \
    bash -c "cp '${bin/\/work\/src-tauri\/target//t}' /out/"
done
echo "staged: $(ls "$STAGE")"

echo "=== running on Windows VM ==="
SSH "powershell -NoProfile -Command \"New-Item -ItemType Directory -Force C:\\ctests | Out-Null\""
FAIL=0
for exe in "$STAGE"/*.exe; do
  name="$(basename "$exe")"
  SCP "$exe" "$SSH_DEST:C:/ctests/$name"
  echo "--- $name"
  if ! SSH "C:\\ctests\\$name --test-threads 4"; then
    FAIL=1
  fi
done
exit $FAIL
