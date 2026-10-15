import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
const source = fs.readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
const fn = source.match(/^build_and_launch_dev\(\) \{[\s\S]*?^\}/m)[0];
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
for (const failure of [0, 42]) {
  test(`dev releases gateway before prebuilt launch; cargo exit=${failure}`, () => {
    const script = `set -euo pipefail
${fn.replace('exec bash ./build.sh', 'launch')}
CODETERM_WEB_DIST= CT_DEV_CARGO_PROFILE= CT_DEV_CARGO_FEATURES=
owned=1
gateway_cwd="$PWD"
configure_dev_cargo() { CT_DEV_CARGO_PROFILE=dev; CT_DEV_CARGO_FEATURES=opaque; }
dev_conf_path() { printf /dev/null; }
node() { [ "$owned" = 1 ]; [ "$1" = scripts/bundle-cli.js ]; }
cargo() { [ "$owned" = 1 ]; [ "$PWD" = "$gateway_cwd" ]; printf 'cargo %s\\n' "$*"; return ${failure}; }
git() { printf opaque-sha; }
write_dev_provenance() { [ "$owned" = 1 ]; echo provenance; }
retain_dev_pdb() { :; }
release_stable_build_path() { [ "$owned" = 1 ]; mapping=0; cd /; }
tauri_debug_dir() { echo opaque-target; }
record_dev_target() { [ "$mapping" = 0 ]; }
complete_build_gateway_before_handoff() { [ "$mapping" = 0 ]; owned=0; echo released; }
launch() { [ "$owned" = 0 ]; [ "$PWD" != "$gateway_cwd" ]; printf 'launch %s\\n' "$*"; }
build_and_launch_dev --wait --notify-tab opaque-tab`;
    const result = spawnSync(bash, ['-c', script], { encoding: 'utf8' });
    assert.equal(result.status, failure, result.stderr);
    assert.match(result.stdout, /cargo build .*--bin codeterm --profile dev --no-default-features --features opaque/);
    if (failure) assert.doesNotMatch(result.stdout, /provenance|released|launch/);
    else assert.match(result.stdout, /provenance\nreleased\n[^\n]*\nlaunch dev-prebuilt --wait --notify-tab opaque-tab/);
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
