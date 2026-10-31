#!/usr/bin/env bash
set -euo pipefail
[[ ${CI:-} == true && ${DOMIOS_DOCKER_SMOKE:-} == 1 && $(uname -s) == Linux ]] || {
  echo 'Production-profile smoke inputs require explicit Linux CI mode' >&2
  exit 2
}
case "$(uname -m):${CARGO_BUILD_TARGET:-}" in
  x86_64:x86_64-unknown-linux-gnu|aarch64:aarch64-unknown-linux-gnu) ;;
  *) echo 'A matching native Linux target is required' >&2; exit 2 ;;
esac
logs="target/docker-smoke/logs/$CARGO_BUILD_TARGET"
mkdir -p "$logs"
pnpm build
cargo build --locked --release --manifest-path cli/Cargo.toml \
  --target "$CARGO_BUILD_TARGET" --message-format=json-render-diagnostics > "$logs/cli.jsonl"
CODETERM_SERVER_CLI_SHA256=$(node scripts/docker/package-ci.mjs cli-digest "$logs/cli.jsonl")
export CODETERM_SERVER_CLI_SHA256
cargo build --locked --release --manifest-path src-tauri/Cargo.toml \
  -p domios-server --bin domios-server --no-default-features \
  --features domios-server/embeddings,domios-server/mem-postgres \
  --target "$CARGO_BUILD_TARGET" --message-format=json-render-diagnostics > "$logs/server.jsonl"
cargo tree --locked --manifest-path src-tauri/Cargo.toml -p domios-server \
  --target "$CARGO_BUILD_TARGET" --no-default-features \
  --features domios-server/embeddings,domios-server/mem-postgres \
  --edges normal,build --prefix none --format '{p}' > "$logs/closure.txt"
node scripts/docker/package-ci.mjs package "$CARGO_BUILD_TARGET" "$logs"
