#!/usr/bin/env bash
# Caller: CI build-gateway.yml; skill build
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TEST_ROOT="$ROOT/.research/build-perf/gateway-policy-$$"
export CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$TEST_ROOT/state"
mkdir -p "$TEST_ROOT"
cleanup() {
  local rc="$?"
  [ "$rc" -ne 0 ] || rm -rf "$TEST_ROOT"
  return "$rc"
}
trap cleanup EXIT

expect_log() {
  local pattern="$1" source="${2:-}" body
  if [ -n "$source" ]; then body="$(cat "$source")"; else body="$(cat)"; source="stdin"; fi
  grep -q "$pattern" <<< "$body" && return 0
  echo "guard assertion failed: expected '$pattern' in $source; contents:" >&2
  printf '%s
' "$body" >&2
  return 1
}

if CODETERM_BUILD_GATEWAY_DIR="$TEST_ROOT/alternate" ./build.sh gateway-fake spoof 0 success \
    > "$TEST_ROOT/alternate-gateway.log" 2>&1; then
  echo "alternate production gateway scope unexpectedly passed" >&2
  exit 1
fi
expect_log "cannot redirect the machine build gateway" "$TEST_ROOT/alternate-gateway.log"

if CT_BUILD_GATEWAY_SCOPE=fixture ./build.sh gateway-fake spoof 0 success \
    > "$TEST_ROOT/forged-fixture-scope.log" 2>&1; then
  echo "forged internal fixture scope unexpectedly passed" >&2
  exit 1
fi
expect_log "internal to fake gateway commands" "$TEST_ROOT/forged-fixture-scope.log"

if scripts/qa-prep.sh > "$TEST_ROOT/direct-qa-prep.log" 2>&1; then
  echo "direct qa-prep unexpectedly bypassed the gateway" >&2
  exit 1
fi
expect_log "internal build.sh child" "$TEST_ROOT/direct-qa-prep.log"

if CODETERM_BUILD_GATEWAY_ACTIVE=1 CODETERM_BUILD_GATEWAY_JOB_ID=forged scripts/qa-prep.sh \
    > "$TEST_ROOT/forged-qa-env.log" 2>&1; then
  echo "forged QA environment markers unexpectedly passed" >&2
  exit 1
fi
expect_log "environment gateway markers are not accepted" "$TEST_ROOT/forged-qa-env.log"

source scripts/build-gateway.sh
node --test scripts/cache-retention.test.mjs
forged_parent_start="$(ct_gateway_process_start_token "$$")"
if scripts/qa-prep.sh --gateway-context forged "$$" "$forged_parent_start" qa-prep --probe-only \
    > "$TEST_ROOT/forged-qa-parent.log" 2>&1; then
  echo "non-build.sh QA parent unexpectedly passed" >&2
  exit 1
fi
expect_log "parent is not build.sh" "$TEST_ROOT/forged-qa-parent.log"

CT_BUILD_JOB_ID=qa-child ./build.sh gateway-policy-probe qa-child > "$TEST_ROOT/qa-child.log"
expect_log "parent/lock/worktree/command/source verified" "$TEST_ROOT/qa-child.log"

if CODETERM_EXPECTED_BUILD_SHA=definitely-wrong ./build.sh gateway-policy-probe release-source \
    > "$TEST_ROOT/release-mismatch.log" 2>&1; then
  echo "release source mismatch unexpectedly passed" >&2
  exit 1
fi
expect_log "release source mismatch" "$TEST_ROOT/release-mismatch.log"

bash scripts/check-release-source-clean.sh
bash scripts/check-release-signing.sh

CT_BUILD_JOB_ID=release-policy ./build.sh gateway-policy-probe release-target > "$TEST_ROOT/release-policy.log"
expect_log "incremental=0" "$TEST_ROOT/release-policy.log"
expect_log "/targets/release/release-policy" "$TEST_ROOT/release-policy.log"

FAKE_BIN="$TEST_ROOT/fake-bin"
mkdir -p "$FAKE_BIN"
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$SCCACHE_CACHE_SIZE" > "$CODETERM_SCCACHE_CAP_RECEIPT"\ncase "${1:-}" in --show-stats) echo fake-stats;; --version) echo sccache-fixture;; esac\n' > "$FAKE_BIN/sccache"
cat > "$FAKE_BIN/node" <<'NODE'
#!/usr/bin/env bash
case "$1" in */sccache-server-probe.mjs) exit 0 ;; esac
exec "$FIXTURE_REAL_NODE" "$@"
NODE
chmod +x "$FAKE_BIN/sccache" "$FAKE_BIN/node"
env -u SCCACHE_CACHE_SIZE FIXTURE_REAL_NODE="$(command -v node)" CT_BUILD_JOB_ID=worktree-policy CODETERM_SCCACHE_CAP_RECEIPT="$TEST_ROOT/sccache-cap" PATH="$FAKE_BIN:$PATH" \
  ./build.sh gateway-policy-probe worktree-target > "$TEST_ROOT/worktree-policy.log"
expect_log "incremental=0" "$TEST_ROOT/worktree-policy.log"
expect_log "/targets/exceptional/" "$TEST_ROOT/worktree-policy.log"
expect_log "wrapper=sccache" "$TEST_ROOT/worktree-policy.log"
[ "$(cat "$TEST_ROOT/sccache-cap")" = 10G ]
node - <<'JS' "$TEST_ROOT/state"
const fs = require('node:fs');
const path = require('node:path');
const root = path.join(process.argv[2], 'targets', 'exceptional');
const markers = fs.readdirSync(root).map(name => JSON.parse(fs.readFileSync(path.join(root, name, '.codeterm-cache.json'))));
if (!markers.some(marker => marker.checkout && marker.lastUsed > 0 && marker.sizeBytes >= 0)) {
  throw new Error('gateway preparation did not persist retention metadata');
}
JS

cli_flow="$(sed -n '/^  cli)/,/^    ;;/p' build.sh)"
expect_log 'cargo build --release --manifest-path cli/Cargo.toml' <<< "$cli_flow"
expect_log 'write_cli_docs' <<< "$cli_flow"
expect_log 'install_cli_to "$CT_DATA_DIR/bin"' <<< "$cli_flow"
if grep -q '\.codeterm/bin' <<< "$cli_flow"; then
  echo "development CLI builds must not overwrite the production CLI" >&2
  exit 1
fi
if grep -q 'enter_build_gateway' <<< "$cli_flow"; then
  echo "CLI build must not wait behind the independent app/dev gateway" >&2
  exit 1
fi

dev_flow="$(sed -n '/^  dev)/,/^    ;;/p' build.sh)"
expect_log 'install_cli_to "$CT_DATA_DIR/bin"' <<< "$dev_flow"
expect_log 'export PATH="$CT_DATA_DIR/bin:$PATH"' <<< "$dev_flow"
expect_log 'export CODETERM_CLI_PATH="$CT_DATA_DIR/bin/' <<< "$dev_flow"
if grep -q '\.codeterm/bin' <<< "$dev_flow"; then
  echo "dev startup must not overwrite the production CLI" >&2
  exit 1
fi
dev_kill_line="$(grep -n 'kill_dev' <<< "$dev_flow" | head -1 | cut -d: -f1)"
dev_gateway_line="$(grep -n 'enter_build_gateway' <<< "$dev_flow" | head -1 | cut -d: -f1)"
[ -n "$dev_kill_line" ] && [ -n "$dev_gateway_line" ] && [ "$dev_kill_line" -lt "$dev_gateway_line" ] || {
  echo "canonical dev restart must release the running dev owner before gateway acquisition" >&2
  exit 1
}

exceptional_flow="$(sed -n '/^prepare_exceptional_worktree_target() {/,/^}/p' build.sh)"
expect_log 'ct_mark_exceptional_target || ' <<< "$exceptional_flow"

worktree_flow="$(sed -n '/^  worktree-check)/,/^    ;;/p' build.sh)"
expect_log 'prepare_exceptional_worktree_target' <<< "$worktree_flow"
if grep -q 'enter_build_gateway' <<< "$worktree_flow"; then
  echo "isolated worktree check must not wait behind the shared dev gateway" >&2
  exit 1
fi
expect_log 'prepare_dev_frontend_dist "$(pwd -P)"' <<< "$worktree_flow"
expect_log 'cargo check --manifest-path src-tauri/Cargo.toml --all-targets' <<< "$worktree_flow"
worktree_preflight_line="$(grep -n 'prepare_dev_frontend_dist' <<< "$worktree_flow" | head -1 | cut -d: -f1)"
worktree_cargo_line="$(grep -n 'cargo check' <<< "$worktree_flow" | head -1 | cut -d: -f1)"
[ -n "$worktree_preflight_line" ] && [ -n "$worktree_cargo_line" ] && [ "$worktree_preflight_line" -lt "$worktree_cargo_line" ] || {
  echo "worktree check must prepare dev frontend resources before cargo" >&2
  exit 1
}

check_tests_flow="$(sed -n '/^  check-tests)/,/^    ;;/p' build.sh)"
expect_log 'prepare_exceptional_worktree_target' <<< "$check_tests_flow"
expect_log 'prepare_dev_frontend_dist "$(pwd -P)"' <<< "$check_tests_flow"
expect_log 'prepare_bundle_stubs' <<< "$check_tests_flow"
expect_log 'cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets --no-run' <<< "$check_tests_flow"
expect_log 'cargo test --manifest-path cli/Cargo.toml --all-targets --no-run' <<< "$check_tests_flow"
if grep -q 'enter_build_gateway' <<< "$check_tests_flow"; then
  echo "check-tests must compile test profiles without waiting behind the shared dev gateway" >&2
  exit 1
fi

mkdir -p "$TEST_ROOT/no-cargo"
printf '#!/usr/bin/env bash\necho "cargo invoked" >&2\nexit 97\n' > "$TEST_ROOT/no-cargo/cargo"
chmod +x "$TEST_ROOT/no-cargo/cargo"
check_tests_wait_status=0
PATH="$TEST_ROOT/no-cargo:$PATH" ./build.sh check-tests --wait-build \
  > "$TEST_ROOT/check-tests-wait.log" 2>&1 || check_tests_wait_status=$?
[ "$check_tests_wait_status" -eq 2 ] || {
  echo "check-tests --wait-build must refuse with exit 2, got $check_tests_wait_status" >&2
  cat "$TEST_ROOT/check-tests-wait.log" >&2
  exit 1
}
expect_log "check-tests does not use the build gateway" "$TEST_ROOT/check-tests-wait.log"
if grep -q "cargo invoked" "$TEST_ROOT/check-tests-wait.log"; then
  echo "check-tests started cargo before refusing --wait-build" >&2
  exit 1
fi

test_one_flow="$(sed -n '/^  test-one)/,/^  verify)/p' build.sh)"
expect_log 'types) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-types/Cargo.toml -p codeterm-types --all-targets "$test_filter"' <<< "$test_one_flow"
expect_log 'scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm-service-core --lib "$test_filter"' <<< "$test_one_flow"
expect_log 'cli) scripts/test-capped.sh --manifest-path cli/Cargo.toml --all-targets "$test_filter"' <<< "$test_one_flow"

nextest_flow="$(sed -n '/^  nextest)/,/^  verify)/p' build.sh)"
expect_log 'run_nextest_scope "${@:2}"' <<< "$nextest_flow"
nextest_runner="$(sed -n '/^run_nextest_scope()/,/^}/p' build.sh)"
expect_log 'enter_build_gateway' <<< "$nextest_runner"
expect_log 'cargo nextest list --manifest-path' <<< "$nextest_runner"
expect_log 'CARGO_TEST_CMD="cargo nextest run" CARGO_TEST_BUILD_CMD=""' <<< "$nextest_runner"
expect_log 'scripts/test-capped.sh' <<< "$nextest_runner"

verify_flow="$(sed -n '/^  verify)/,/^    ;;/p' build.sh)"
verify_coverage_checks_typescript() {
  grep -q 'node node_modules/typescript/bin/tsc --noEmit' <<< "$verify_flow"
}
verify_coverage_runs_cli_tests() {
  grep -q 'scripts/test-capped.sh --manifest-path cli/Cargo.toml' <<< "$verify_flow"
}
verify_coverage_checks_typescript
verify_coverage_runs_cli_tests

embed_bundle_flow="$(sed -n '/^ensure_real_embed_bundle()/,/^}/p' build.sh)"
expect_log 'CODETERM_DEV=1 node scripts/bundle-embed.js' <<< "$embed_bundle_flow" || {
  echo "dev must ad-hoc sign codeterm-embed without a Developer ID identity" >&2
  exit 1
}

./build.sh gateway-status > "$TEST_ROOT/status.json"
node - <<'JS' "$TEST_ROOT/status.json"
const status = require(process.argv[2]);
if (status.state !== "idle" || status.queue.length !== 0 || status.active !== null) throw new Error("policy probes left gateway occupied");
for (const id of ["qa-child", "release-policy", "worktree-policy"]) {
  const job = status.recent.find((item) => item.job_id === id);
  if (job?.state !== "completed") throw new Error(`${id} did not complete through gateway`);
}
JS

# tauri bundles every src/bin entry as a binary, so a module directory there breaks every release bundle.
bin_dirs="$(find src-tauri/src/bin -mindepth 1 -maxdepth 1 -type d)"
if [ -n "$bin_dirs" ]; then
  echo "guard assertion failed: directories under src-tauri/src/bin become bogus bundle binaries: $bin_dirs" >&2
  exit 1
fi
# A positional build.ps1 parameter swallows the build command (`.\build.ps1 release-inputs`).
if ! grep -q 'PositionalBinding = \$false' build.ps1 || grep -q 'Position *=' build.ps1; then
  echo "guard assertion failed: build.ps1 parameters must stay named-only" >&2
  exit 1
fi

echo "PASS build gateway serialization, restart ordering, CLI independence, cache, release freshness, and SHA policy"
