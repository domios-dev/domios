#!/usr/bin/env bash
# Caller: CI cloud-image.yml; skill github-release
# Builds the QA-profile Linux headless archive for docker/cloud.Dockerfile (ARCHIVE_SOURCE=ci-smoke).
set -euo pipefail
set +x

[[ ${CI:-} == true && ${DOMIOS_CLOUD_CANDIDATE:-} == 1 && $(uname -s) == Linux ]] || {
  echo 'Cloud candidate QA inputs require explicit Linux CI mode (CI=true DOMIOS_CLOUD_CANDIDATE=1)' >&2
  exit 2
}
case "$(uname -m):${CARGO_BUILD_TARGET:-}" in
  x86_64:x86_64-unknown-linux-gnu|aarch64:aarch64-unknown-linux-gnu) ;;
  *) echo 'A matching native Linux CARGO_BUILD_TARGET is required' >&2; exit 2 ;;
esac

for secret_key in DOMIOS_QA_PROJECT_ID DOMIOS_QA_FIREBASE_API_KEY DOMIOS_QA_FIREBASE_APP_ID; do
  if [[ -z "${!secret_key:-}" ]]; then
    echo "::error::Missing required QA Firebase build secret: ${secret_key}" >&2
    exit 2
  fi
done

command -v "${OBJCOPY:-objcopy}" >/dev/null 2>&1 || {
  echo "Missing required objcopy tool (${OBJCOPY:-objcopy}) to strip QA cloud binaries" >&2
  exit 2
}

rm -rf target/cloud-qa
logs="target/cloud-qa/logs/$CARGO_BUILD_TARGET"
mkdir -p "$logs"

pnpm build

cargo build --locked --manifest-path cli/Cargo.toml \
  --target "$CARGO_BUILD_TARGET" --message-format=json-render-diagnostics > "$logs/cli.jsonl"

CODETERM_SERVER_CLI_SHA256=$(node scripts/docker/package-cloud-qa.mjs cli-digest "$logs/cli.jsonl" "$CARGO_BUILD_TARGET")
export CODETERM_SERVER_CLI_SHA256

DOMIOS_ACCOUNT_TARGET=qa \
  cargo build --locked --manifest-path src-tauri/Cargo.toml \
    -p domios-server --bin domios-server --no-default-features \
    --features domios-server/embeddings,domios-server/mem-postgres \
    --target "$CARGO_BUILD_TARGET" --message-format=json-render-diagnostics > "$logs/server.jsonl"

cargo tree --locked --manifest-path src-tauri/Cargo.toml -p domios-server \
  --target "$CARGO_BUILD_TARGET" --no-default-features \
  --features domios-server/embeddings,domios-server/mem-postgres \
  --edges normal,build --prefix none --format '{p}' > "$logs/closure.txt"

node scripts/docker/package-cloud-qa.mjs package "$CARGO_BUILD_TARGET" "$logs"

version=$(node -p "require('./src-tauri/tauri.conf.json').version")
source_sha=$(git rev-parse HEAD)
platform=$([[ "$CARGO_BUILD_TARGET" == aarch64-* ]] && echo linux-aarch64 || echo linux-x86_64)
archive="domios-headless-v${version}-${platform}.tar.gz"
archives="target/cloud-qa/archives"
test "$(jq -r .source_sha "${archives}/${archive}.manifest.json")" = "${source_sha}"
(cd "${archives}" && sha256sum -c "${archive}.sha256")
