#!/usr/bin/env bash
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

if CODETERM_BUILD_GATEWAY_DIR="$TEST_ROOT/alternate" ./build.sh gateway-fake spoof 0 success \
    > "$TEST_ROOT/alternate-gateway.log" 2>&1; then
  echo "alternate production gateway scope unexpectedly passed" >&2
  exit 1
fi
grep -q "cannot redirect the machine build gateway" "$TEST_ROOT/alternate-gateway.log"

if CT_BUILD_GATEWAY_SCOPE=fixture ./build.sh gateway-fake spoof 0 success \
    > "$TEST_ROOT/forged-fixture-scope.log" 2>&1; then
  echo "forged internal fixture scope unexpectedly passed" >&2
  exit 1
fi
grep -q "internal to fake gateway commands" "$TEST_ROOT/forged-fixture-scope.log"

if scripts/qa-prep.sh > "$TEST_ROOT/direct-qa-prep.log" 2>&1; then
  echo "direct qa-prep unexpectedly bypassed the gateway" >&2
  exit 1
fi
grep -q "internal build.sh child" "$TEST_ROOT/direct-qa-prep.log"

if CODETERM_BUILD_GATEWAY_ACTIVE=1 CODETERM_BUILD_GATEWAY_JOB_ID=forged scripts/qa-prep.sh \
    > "$TEST_ROOT/forged-qa-env.log" 2>&1; then
  echo "forged QA environment markers unexpectedly passed" >&2
  exit 1
fi
grep -q "environment gateway markers are not accepted" "$TEST_ROOT/forged-qa-env.log"

source scripts/build-gateway.sh
forged_parent_start="$(ct_gateway_process_start_token "$$")"
if scripts/qa-prep.sh --gateway-context forged "$$" "$forged_parent_start" qa-prep --probe-only \
    > "$TEST_ROOT/forged-qa-parent.log" 2>&1; then
  echo "non-build.sh QA parent unexpectedly passed" >&2
  exit 1
fi
grep -q "parent is not build.sh" "$TEST_ROOT/forged-qa-parent.log"

CT_BUILD_JOB_ID=qa-child ./build.sh gateway-policy-probe qa-child > "$TEST_ROOT/qa-child.log"
grep -q "parent/lock/worktree/command/source verified" "$TEST_ROOT/qa-child.log"

if CODETERM_EXPECTED_BUILD_SHA=definitely-wrong ./build.sh gateway-policy-probe release-source \
    > "$TEST_ROOT/release-mismatch.log" 2>&1; then
  echo "release source mismatch unexpectedly passed" >&2
  exit 1
fi
grep -q "release source mismatch" "$TEST_ROOT/release-mismatch.log"

bash scripts/check-release-source-clean.sh
bash scripts/check-release-signing.sh

CT_BUILD_JOB_ID=release-policy ./build.sh gateway-policy-probe release-target > "$TEST_ROOT/release-policy.log"
grep -q "incremental=0" "$TEST_ROOT/release-policy.log"
grep -q "/targets/release/release-policy" "$TEST_ROOT/release-policy.log"

FAKE_BIN="$TEST_ROOT/fake-bin"
mkdir -p "$FAKE_BIN"
printf '#!/usr/bin/env bash\ncase "${1:-}" in --show-stats) echo fake-stats;; --version) echo sccache-fixture;; esac\n' > "$FAKE_BIN/sccache"
chmod +x "$FAKE_BIN/sccache"
CT_BUILD_JOB_ID=worktree-policy PATH="$FAKE_BIN:$PATH" \
  ./build.sh gateway-policy-probe worktree-target > "$TEST_ROOT/worktree-policy.log"
grep -q "incremental=0" "$TEST_ROOT/worktree-policy.log"
grep -q "/targets/exceptional/" "$TEST_ROOT/worktree-policy.log"
grep -q "wrapper=sccache" "$TEST_ROOT/worktree-policy.log"

cli_flow="$(sed -n '/^  cli)/,/^    ;;/p' build.sh)"
grep -q 'cargo build --release --manifest-path cli/Cargo.toml' <<< "$cli_flow"
grep -q 'write_cli_docs' <<< "$cli_flow"
grep -q 'install_cli_to "$CT_DATA_DIR/bin"' <<< "$cli_flow"
if grep -q '\.codeterm/bin' <<< "$cli_flow"; then
  echo "development CLI builds must not overwrite the production CLI" >&2
  exit 1
fi
if grep -q 'enter_build_gateway' <<< "$cli_flow"; then
  echo "CLI build must not wait behind the independent app/dev gateway" >&2
  exit 1
fi

dev_flow="$(sed -n '/^  dev)/,/^    ;;/p' build.sh)"
grep -q 'install_cli_to "$CT_DATA_DIR/bin"' <<< "$dev_flow"
grep -q 'export PATH="$CT_DATA_DIR/bin:$PATH"' <<< "$dev_flow"
grep -q 'export CODETERM_CLI_PATH="$CT_DATA_DIR/bin/' <<< "$dev_flow"
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

embed_bundle_flow="$(sed -n '/^ensure_real_embed_bundle()/,/^}/p' build.sh)"
grep -q 'CODETERM_DEV=1 node scripts/bundle-embed.js' <<< "$embed_bundle_flow" || {
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

echo "PASS build gateway serialization, restart ordering, CLI independence, cache, release freshness, and SHA policy"
