#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TEST_ROOT="$ROOT/.research/build-perf/dev-resource-preflight-$$"
FIXTURE="$TEST_ROOT/worktree"
FAKE_BIN="$TEST_ROOT/fake-bin"
export CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$TEST_ROOT/gateway"
mkdir -p "$FIXTURE/src-tauri" "$FAKE_BIN"

cleanup() {
  local rc="$?"
  if [ -n "${JUNCTION_PATH:-}" ]; then
    CT_FIXTURE_PATH="$(cygpath -w "$JUNCTION_PATH")" powershell.exe -NoProfile -Command \
      'if (Test-Path -LiteralPath $env:CT_FIXTURE_PATH) { Remove-Item -LiteralPath $env:CT_FIXTURE_PATH -Force }' >/dev/null 2>&1 || true
  fi
  [ "$rc" -ne 0 ] || rm -rf "$TEST_ROOT"
  return "$rc"
}
trap cleanup EXIT

cat > "$FIXTURE/src-tauri/tauri.conf.json" <<'JSON'
{
  "build": {
    "frontendDist": "../dist"
  }
}
JSON

cat > "$FAKE_BIN/cargo" <<EOF
#!/usr/bin/env bash
touch "$TEST_ROOT/cargo-started"
exit 91
EOF
chmod +x "$FAKE_BIN/cargo"

CT_BUILD_JOB_ID=dev-resource-preflight PATH="$FAKE_BIN:$PATH" \
  ./build.sh gateway-policy-probe dev-resources "$FIXTURE" > "$TEST_ROOT/preflight.log"

[ -d "$FIXTURE/dist" ] || { echo "worktree-local frontendDist was not created" >&2; exit 1; }
[ ! -e "$TEST_ROOT/cargo-started" ] || { echo "cargo started before dev resource preflight completed" >&2; exit 1; }
grep -q "worktree-local frontendDist ready" "$TEST_ROOT/preflight.log"

node - <<'JS' build.sh
const fs = require("node:fs");
const source = fs.readFileSync(process.argv[2], "utf8");
const dev = source.slice(source.indexOf("  dev)"), source.indexOf("  e2e)"));
const gateway = dev.indexOf("enter_build_gateway");
const preflight = dev.indexOf("prepare_dev_frontend_dist");
const cargo = dev.indexOf("cargo build");
if (gateway < 0 || preflight < 0 || cargo < 0 || !(gateway < preflight && preflight < cargo)) {
  throw new Error("dev resource preflight must run inside the gateway before the first cargo process");
}
JS

OUTSIDE="$TEST_ROOT/outside"
mkdir -p "$OUTSIDE"
cat > "$FIXTURE/src-tauri/tauri.conf.json" <<JSON
{
  "build": {
    "frontendDist": "../../outside"
  }
}
JSON
if node scripts/dev-resource-preflight.mjs "$FIXTURE" > "$TEST_ROOT/outside.log" 2>&1; then
  echo "external frontendDist unexpectedly passed" >&2
  exit 1
fi
grep -q "must stay inside this checkout" "$TEST_ROOT/outside.log"

assert_redirect_rejected() {
  local name="$1" configured="$2" outside_leaf="$3"
  cat > "$FIXTURE/src-tauri/tauri.conf.json" <<JSON
{
  "build": {
    "frontendDist": "$configured"
  }
}
JSON
  if node scripts/dev-resource-preflight.mjs "$FIXTURE" > "$TEST_ROOT/$name.log" 2>&1; then
    echo "$name redirect unexpectedly passed" >&2
    exit 1
  fi
  grep -Eq "symlink, junction, or reparse point|resolves outside this checkout" "$TEST_ROOT/$name.log"
  [ ! -e "$outside_leaf" ] || { echo "$name created content outside the checkout" >&2; exit 1; }
}

DIRECT_OUTSIDE="$TEST_ROOT/direct-outside"
ANCESTOR_OUTSIDE="$TEST_ROOT/ancestor-outside"
mkdir -p "$DIRECT_OUTSIDE" "$ANCESTOR_OUTSIDE"

SYMLINKS_AVAILABLE=1
if [[ "${OSTYPE:-}" == msys* || "${OSTYPE:-}" == cygwin* || "${OSTYPE:-}" == win32* ]]; then
  DIRECT_LINK_WIN="$(cygpath -w "$FIXTURE/direct-link")"
  DIRECT_OUTSIDE_WIN="$(cygpath -w "$DIRECT_OUTSIDE")"
  if ! CT_FIXTURE_LINK="$DIRECT_LINK_WIN" CT_FIXTURE_TARGET="$DIRECT_OUTSIDE_WIN" \
    powershell.exe -NoProfile -Command \
    'New-Item -ItemType SymbolicLink -Path $env:CT_FIXTURE_LINK -Target $env:CT_FIXTURE_TARGET -ErrorAction Stop | Out-Null' \
    >/dev/null 2>&1; then
    SYMLINKS_AVAILABLE=0
    echo "SKIP privileged native Windows symlink fixtures"
  fi
elif ! ln -s "$DIRECT_OUTSIDE" "$FIXTURE/direct-link" 2>/dev/null; then
  SYMLINKS_AVAILABLE=0
  if [[ "${OSTYPE:-}" != msys* && "${OSTYPE:-}" != cygwin* && "${OSTYPE:-}" != win32* ]]; then
    echo "native symlink fixture could not be created" >&2
    exit 1
  fi
fi

if [ "$SYMLINKS_AVAILABLE" -eq 1 ]; then
  assert_redirect_rejected direct-symlink "../direct-link" "$DIRECT_OUTSIDE/created"
  if [[ "${OSTYPE:-}" == msys* || "${OSTYPE:-}" == cygwin* || "${OSTYPE:-}" == win32* ]]; then
    CT_FIXTURE_LINK="$(cygpath -w "$FIXTURE/ancestor-link")" CT_FIXTURE_TARGET="$(cygpath -w "$ANCESTOR_OUTSIDE")" \
      powershell.exe -NoProfile -Command \
      'New-Item -ItemType SymbolicLink -Path $env:CT_FIXTURE_LINK -Target $env:CT_FIXTURE_TARGET -ErrorAction Stop | Out-Null'
  else
    ln -s "$ANCESTOR_OUTSIDE" "$FIXTURE/ancestor-link"
  fi
  assert_redirect_rejected symlink-ancestor "../ancestor-link/missing-leaf" "$ANCESTOR_OUTSIDE/missing-leaf"
fi

if [[ "${OSTYPE:-}" == msys* || "${OSTYPE:-}" == cygwin* || "${OSTYPE:-}" == win32* ]]; then
  JUNCTION_OUTSIDE="$TEST_ROOT/junction-outside"
  mkdir -p "$JUNCTION_OUTSIDE"
  JUNCTION_PATH="$FIXTURE/junction-link"
  CT_FIXTURE_LINK="$(cygpath -w "$JUNCTION_PATH")" CT_FIXTURE_TARGET="$(cygpath -w "$JUNCTION_OUTSIDE")" \
    powershell.exe -NoProfile -Command \
    'New-Item -ItemType Junction -Path $env:CT_FIXTURE_LINK -Target $env:CT_FIXTURE_TARGET -ErrorAction Stop | Out-Null'
  assert_redirect_rejected junction-ancestor "../junction-link/missing-leaf" "$JUNCTION_OUTSIDE/missing-leaf"
  CT_FIXTURE_PATH="$(cygpath -w "$JUNCTION_PATH")" powershell.exe -NoProfile -Command \
    'Remove-Item -LiteralPath $env:CT_FIXTURE_PATH -Force -ErrorAction Stop'
  JUNCTION_PATH=""
fi

node - <<'JS' \
  build.sh \
  src-tauri/tauri.conf.json \
  .agents/skills/github-release/scripts/build-mac.sh \
  scripts/release-macos-x86_64.sh \
  .codeterm/build-machine/scripts/build-linux.sh \
  .github/workflows/release-linux.yml \
  .github/workflows/release-windows.yml
const fs = require("node:fs");
const source = fs.readFileSync(process.argv[2], "utf8");
const build = source.slice(source.indexOf("  build)"), source.indexOf("  qa-prep)"));
const prepare = source.slice(
  source.indexOf("prepare_release_bundle_inputs()"),
  source.indexOf("\n}\n", source.indexOf("prepare_release_bundle_inputs()")) + 2,
);
if (!prepare.includes("pnpm build")) throw new Error("release inputs must build the frontend");
if (!source.includes("  release-inputs)")) throw new Error("release-inputs operation is missing");
if (!build.includes("pnpm tauri build")) throw new Error("release build no longer invokes Tauri");
const config = JSON.parse(fs.readFileSync(process.argv[3], "utf8"));
if (config.build.beforeBuildCommand) throw new Error("Tauri must not own release input preparation");
for (const path of process.argv.slice(4)) {
  const entrypoint = fs.readFileSync(path, "utf8");
  if (!entrypoint.includes("release-inputs")) {
    throw new Error(`${path} bypasses target-aware release input preparation`);
  }
}
const windowsRelease = fs.readFileSync(process.argv.at(-1), "utf8");
if (!windowsRelease.includes("Cross-target LIB contains host-architecture paths")) {
  throw new Error("Windows ARM64 release must reject host library paths before compilation");
}
JS

echo "PASS gateway dev preflight validates ancestors and release entrypoints stage target-aware bundle inputs"
