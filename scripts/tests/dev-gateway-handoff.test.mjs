import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
const source = fs.readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
const fn = source.match(/^build_and_launch_dev\(\) \{[\s\S]*?^\}/m)[0];
const clientFn = source.match(/^build_dev_client\(\) \{[\s\S]*?^\}/m)[0];
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
for (const failure of [0, 42]) {
  test(`dev releases gateway before prebuilt launch; cargo exit=${failure}`, () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-gateway-'));
    fs.mkdirSync(path.join(directory, '.domios-dev-build'));
    fs.writeFileSync(path.join(directory, '.domios-dev-build/cli.jsonl'), 'cli-artifact');
    try {
    const script = `set -euo pipefail
${clientFn}
${fn.replace('exec bash ./build.sh', 'launch')}
CODETERM_WEB_DIST= CT_DEV_CARGO_PROFILE= CT_DEV_CARGO_FEATURES=
CODETERM_BUILD_SHA=opaque-sha
CODETERM_BUILD_HASH=opaque-short
built_cli_artifact=opaque-dev-cli
built_cli_hash=opaque-hash CT_VITE_PORT=1520
owned=1
named_dev_isolated() { return 1; }
gateway_cwd="$PWD"
configure_dev_cargo() { CT_DEV_CARGO_PROFILE=dev; CT_DEV_CARGO_FEATURES=opaque; }
dev_conf_path() { printf /dev/null; }
node() {
  [ "$owned" = 1 ]
  if [ "$1" = scripts/bundle-cli.js ]; then [ "$2" = "$built_cli_artifact" ]; return; fi
  [ "$1" = scripts/server-artifact.mjs ] && [ "$2" = stage-dev ]
  [ "$(cat "$3")" = server-artifact ] && [ "$(cat "$4")" = server-tree ] && [ "$(cat "$5")" = cli-artifact ]
  [ "$6" = "$gateway_cwd" ] && [ "$7" = "$(dev_resource_dir)" ] && [ "$8" = "$(dev_built_app_path)" ] && [ "$9" = "$CODETERM_BUILD_SHA" ]
  echo staged
}
cargo() {
  [ "$owned" = 1 ] && [ "$PWD" = "$gateway_cwd" ]
  printf 'cargo %s\\n' "$*" >&2
  if [ ${failure} != 0 ]; then return ${failure}; fi
  case "$*" in
    'build --manifest-path src-tauri/Cargo.toml -p domios-server '*) echo server-artifact ;;
    'tree --manifest-path src-tauri/Cargo.toml -p domios-server '*) echo server-tree ;;
    'build --manifest-path src-tauri/Cargo.toml -p codeterm '*) echo client-artifact ;;
    *) return 93 ;;
  esac
}
git() { printf opaque-sha; }
build_dev_frontend() { [ "$owned" = 1 ]; echo frontend; }
write_dev_provenance() { [ "$owned" = 1 ]; echo provenance; }
retain_dev_pdb() { :; }
release_stable_build_path() { [ "$owned" = 1 ]; mapping=0; cd /; }
tauri_debug_dir() { printf '%s' '${directory.replaceAll('\\', '/')}'; }
dev_resource_dir() { echo opaque-resources; }
dev_built_app_path() { echo opaque-client; }
record_dev_target() { [ "$mapping" = 0 ]; }
complete_build_gateway_before_handoff() { [ "$mapping" = 0 ]; owned=0; echo released; }
launch() { [ "$owned" = 0 ]; [ "$PWD" != "$gateway_cwd" ]; [ "$CT_DEV_HANDOFF_SHA" = "$CODETERM_BUILD_SHA" ]; [ "$CT_DEV_HANDOFF_HASH" = "$CODETERM_BUILD_HASH" ]; printf 'launch %s\\n' "$*"; }
build_and_launch_dev --wait --notify-tab opaque-tab`;
    const result = spawnSync(bash, ['-c', script], { encoding: 'utf8' });
    assert.equal(result.status, failure, result.stderr);
    assert.match(result.stderr, /cargo build .*--bin domios-server/);
    if (!failure) assert.match(result.stderr, /cargo build .*--bin codeterm --profile dev --no-default-features\n/);
    if (failure) assert.doesNotMatch(result.stdout, /frontend|staged|provenance|released|launch/);
    else assert.match(result.stdout, /frontend\nstaged\nprovenance\nreleased\n[^\n]*\nlaunch dev-prebuilt --wait --notify-tab opaque-tab/);
    } finally { fs.rmSync(directory, { recursive: true, force: true }); }
  });
}
test('prebuilt retains its single provenance validation before stopping dev', () => {
  const prebuilt = source.slice(source.indexOf('  dev-prebuilt)'), source.indexOf('  dev)'));
  assert.ok(prebuilt.indexOf('validate_dev_provenance') < prebuilt.indexOf('    kill_dev'));
  assert.doesNotMatch(prebuilt, /enter_cargo_gateway|enter_build_gateway/);
  assert.doesNotMatch(fn, /sleep|tauri dev/);
});


test('prebuilt launch never runs Cargo preparation after gateway release', () => {
  const prebuilt = source.slice(source.indexOf('  dev-prebuilt)'), source.indexOf('  dev)'));
  const preparation = prebuilt.slice(prebuilt.indexOf('    export PATH='), prebuilt.indexOf('    if [ "$CT_DEV_WAIT" -eq 1 ]; then'));
  const result = spawnSync(bash, ['-c', `set -euo pipefail
CT_DATA_DIR=/tmp CT_CDP_PORT=opaque
DEV_NOTIFY_STAGE=
dev_cli_path() { echo opaque-cli; }
configure_dev_cargo() { :; }
node() { echo unexpected-cargo-preparation; return 91; }
start_web_watch() { echo watch; }
start_dev_static_server() { echo static; }
${preparation}
`], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), 'watch\nstatic');
});
