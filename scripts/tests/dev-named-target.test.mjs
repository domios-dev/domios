import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const source = fs.readFileSync(new URL('../../build.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const helpers = ['dev_build_phase', 'named_dev_target_dir', 'named_dev_isolated', 'configure_named_dev_cache', 'prepare_named_dev_target', 'dev_cli_target_dir', 'build_dev_cli',
  'tauri_debug_dir', 'dev_built_app_path', 'dev_app_path', 'kill_dev', 'dev_provenance_record'].map(name => source.match(new RegExp(`^${name}\\(\\) \\{[\\s\\S]*?^\\}`, 'm'))[0]).join('\n');
const dev = source.slice(source.indexOf('  dev)\n'), source.indexOf('  e2e)\n'));
const kill = source.slice(source.indexOf('  kill)\n'), source.indexOf('  dev-server)\n'));
const prebuilt = source.slice(source.indexOf('  dev-prebuilt)\n'), source.indexOf('    if [ -n "$DEV_NOTIFY_TAB" ]; then\n      trap', source.indexOf('  dev-prebuilt)\n')));

for (const helper of ['start_web_watch', 'start_dev_static_server']) {
  test(`Windows ${helper} leaves the tab job and records the actual child identity`, () => {
    const functions = ['run_outside_tab_job', helper].map(name => source.match(new RegExp(`^${name}\\(\\) \\{[\\s\\S]*?^\\}`, 'm'))[0]).join('\n');
    const result = spawnSync(bash, ['-c', `set -euo pipefail
${functions}
OSTYPE=msys CT_SUFFIX=-fixture CT_VITE_PORT=14997
cygpath() { printf '%s\\n' "$2"; }
web_watch_pidfile() { echo owned-watcher.json; }
dev_static_pidfile() { echo owned-static.json; }
dev_frontend_dir() { echo /fixture/instance/dist; }
kill_web_watch() { :; }
kill_dev_static_server() { :; }
powershell.exe() { printf 'BREAKAWAY=%s\\n' "$BREAKAWAY_COMMAND_LINE"; printf 'ARG=%s\\n' "$@"; }
node() { echo UNSAFE_CONTAINED_NODE; return 1; }
${helper}
`], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /windows-breakaway-run\.ps1/);
    assert.match(result.stdout, /ARG=-ProcessRecord\nARG=owned-(?:watcher|static)\.json/);
    assert.match(result.stdout, helper === 'start_web_watch' ? /BREAKAWAY=.*vite\.js.*build.*--watch/ : /BREAKAWAY=.*dev-static-server\.mjs/);
    assert.doesNotMatch(result.stdout, /UNSAFE_CONTAINED_NODE/);
  });
}

test('background breakaway refusal never launches a contained frontend fallback', () => {
  const helper = source.match(/^run_outside_tab_job\(\) \{[\s\S]*?^\}/m)[0];
  const result = spawnSync(bash, ['-c', `set -euo pipefail
${helper}
OSTYPE=msys
cygpath() { printf '%s\\n' "$2"; }
powershell.exe() { return 250; }
node() { echo UNSAFE_CONTAINED_NODE; }
run_outside_tab_job --background /fixture/owned.json /fixture/frontend.log node /fixture/server.mjs
`], { encoding: 'utf8' });
  assert.equal(result.status, 250, result.stderr);
  assert.match(result.stderr, /refuses detached DEV frontend launch/);
  assert.doesNotMatch(result.stdout, /UNSAFE_CONTAINED_NODE/);
});

for (const [platform, refused] of [['linux', false], ['darwin', false], ['msys', false], ['msys', true]]) {
  test(`daemon shutdown precedes watcher cleanup; platform=${platform}, refused=${refused}`, () => {
    const result = spawnSync(bash, ['-c', `set -euo pipefail
${helpers}
OSTYPE="$1" CT_PORT=17999 CT_VITE_PORT=1529 CT_SUFFIX=-test CT_INSTANCE=test CT_INSTANCE_KEY=test
CT_DATA_DIR=/absent-kill-order-fixture DEV_FRONT_EXTERNAL=1
dev_provenance_record() { echo record; }
dev_app_path() { echo app; }
lsof() { echo 12345; }
screen() { :; }
request_dev_clean_restart() { echo shutdown-request; }
wait_for_dev_exit() { echo pinned-exit; }
node() { echo verified-shutdown; [ "$1" = scripts/dev-instance-processes.mjs ]; [ "$REFUSED" = false ]; }
kill_web_watch() { echo watcher-stop; }
kill_dev_static_server() { :; }
windows_dev_leftover_cleanup() { echo leftover-stop; }
REFUSED="$2"
kill_dev
`, '--', platform, String(refused)], { encoding: 'utf8' });
    const events = result.stdout.trim().split('\n');
    assert.equal(result.status, refused ? 1 : 0, result.stderr);
    assert.deepEqual(events, refused ? ['verified-shutdown']
      : ['verified-shutdown', 'watcher-stop', ...(platform === 'msys' ? ['leftover-stop'] : [])]);
  });
}

test('orphan watcher cleanup stops only this repo and this instance frontend output', () => {
  const helper = source.match(/^kill_orphan_web_watchers\(\) \{[\s\S]*?^\}/m)[0];
  const result = spawnSync(bash, ['-c', `set -euo pipefail
${helper}
OSTYPE=linux
dev_frontend_dir() { echo "$PWD/dist-own"; }
ps() {
  echo "17 node $PWD/node_modules/vite/bin/vite.js build --watch --outDir $PWD/dist-own"
  echo "18 node /foreign/node_modules/vite/bin/vite.js build --watch --outDir /foreign/dist-own"
  echo "19 node $PWD/node_modules/vite/bin/vite.js build --watch --outDir $PWD/dist-other"
  echo "20 node /foreign/node_modules/vite/bin/vite.js build --watch --outDir $PWD/dist-own"
}
lsof() { if [ "$3" = 20 ]; then echo n/foreign; else echo "n$PWD"; fi; }
kill_process_tree() { echo "stopped=$1"; }
kill_orphan_web_watchers
`], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), 'stopped=17');
});

for (const platform of ['linux', 'msys']) test(`scoped frontend cleanup follows the instance stop (${platform})`, () => {
  const result = spawnSync(bash, ['-c', `set -euo pipefail
${helpers}
OSTYPE="$1" CT_PORT=17999 CT_VITE_PORT=14999 CT_INSTANCE=test CT_INSTANCE_KEY=test CT_DATA_DIR=/fixture
DEV_FRONT_EXTERNAL=0
node() { if [ "$2" = --frontend ]; then echo vite-stop; else echo instance-stop; fi; }
kill_web_watch() { echo watcher-stop; }
kill_dev_static_server() { echo static-stop; }
windows_dev_leftover_cleanup() { echo windows-leftovers; }
kill_dev
`, '--', platform], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.stdout.trim().split('\n'), ['instance-stop', 'watcher-stop', 'static-stop',
    platform === 'linux' ? 'vite-stop' : 'windows-leftovers']);
});

for (const instance of ['', 'first-proof', 'second-proof']) {
  test(`shutdown selects the exact instance tree; instance=${instance || 'default'}`, () => {
    const result = spawnSync(bash, ['-c', `set -euo pipefail
${helpers}
OSTYPE=msys
CT_INSTANCE="$1" CT_INSTANCE_KEY="$1" CT_PORT=17999 CT_SUFFIX=
CT_DATA_DIR="$PWD/data"
CARGO_BUILD_TARGET=
if [ -n "$CT_INSTANCE" ]; then CARGO_TARGET_DIR=foreign-target; else unset CARGO_TARGET_DIR; fi
node() { [ "$1" = scripts/dev-instance-processes.mjs ]; printf 'tree=%s\\n' "$2"; }
kill_web_watch() { :; }
kill_dev_static_server() { :; }
windows_dev_leftover_cleanup() { :; }
case kill in
${kill}
esac
`, '--', instance], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const app = result.stdout.split('\n').find(line => line.startsWith('tree=')).slice(5);
    if (instance) {
      assert.ok(app.endsWith('/src-tauri/target/named-dev'), app);
      assert.ok(!app.includes('foreign-target'), app);
    } else {
      assert.equal(app, 'src-tauri/target/debug');
    }
  });
}

for (const platform of ['windows','linux']) for (const wait of [0, 1]) {
  test(`named DEV instances build and start from one incremental checkout target; platform=${platform}, wait=${wait}`, () => {
    const directory = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'dev-named-target-')));
    try {
      const exe = platform === 'windows' ? '.exe' : '';
      const defaultApp = path.join(directory, `src-tauri/target/debug/domios-app${exe}`);
      fs.mkdirSync(path.dirname(defaultApp), { recursive: true });
      fs.writeFileSync(defaultApp, 'default running artifact');
      const result = spawnSync(bash, ['-s', '--', directory, String(wait), platform === 'windows'?'msys':'linux-gnu'], { encoding: 'utf8', input: `set -euo pipefail
${helpers}
cd "$1"
OSTYPE="$3"
CMD=dev CT_DEV_WAIT="$2" CT_PORT=7799 DEV_NOTIFY_TAB= CT_SUFFIX="-target-test-$$"
CARGO_BUILD_TARGET= CODETERM_BUILD_SHA=opaque
unset DOMIOS_QA_PROJECT_ID
dev_health_status() { :; }
dev_health_fields() { :; }
ct_verify_target_dir() { :; }
kill_dev() { :; }
stop_dev_before_replacement() { :; }
enter_cargo_gateway() { :; }
adopt_named_dev_target() { printf '%s\\n' "$CMD:$CT_INSTANCE" >> "$PWD/adoptions"; }
refuse_named_dev_rebuild_while_running() { printf '%s\\n' "$CT_INSTANCE" >> "$PWD/refusal-checks"; }
ct_configure_dev_qa_project() { export DOMIOS_QA_PROJECT_ID=opaque-shared-project; }
prepare_dev_frontend_dist() { :; }
ensure_deps() { :; }
export RUSTC_WRAPPER=inherited-wrapper CODETERM_PORTABLE_COMPILER_CACHE=1 CARGO_INCREMENTAL=0
cargo() {
  [ "$CARGO_TARGET_DIR" = "$PWD/src-tauri/target/named-dev" ]
  [ "$CARGO_INCREMENTAL" = 1 ]
  [ "\${RUSTC_WRAPPER-unset}" = "" ]
  [ "$CODETERM_USE_SCCACHE" = 0 ]
  [ -z "\${CODETERM_PORTABLE_COMPILER_CACHE+x}" ]
  [ "$DOMIOS_QA_PROJECT_ID" = opaque-shared-project ]
}
cli_profile_binary() { echo unused; }
cli_file_sha256_or_status() { echo unused; }
install_cli_to() { :; }
build_and_launch_dev() {
  raw="$(dev_built_app_path)"
  app="$(dev_app_path)"
  mkdir -p "$(dirname "$raw")" "$(dirname "$app")"
  printf '%s' "$CT_INSTANCE" > "$raw"
  if [ "$raw" != "$app" ]; then cp "$raw" "$app"; fi
  printf '%s\\n' "$app" > "$CT_DATA_DIR/built-app"
}
run_prebuilt_preflight() { "$@"; }
refuse_prebuilt() { printf '%s\\n' "$*" >&2; exit 2; }
preflight_dev_prebuilt_runtime() { :; }
validate_dev_provenance() {
  [ "$(dev_app_path)" = "$(cat "$CT_DATA_DIR/built-app")" ]
  [ "$(cat "$(dev_app_path)")" = "$CT_INSTANCE" ]
}
for CT_INSTANCE in meshqa welcomeqa; do
  CT_INSTANCE_KEY="$CT_INSTANCE"
  CT_DATA_DIR="$PWD/data-$CT_INSTANCE"
  mkdir -p "$CT_DATA_DIR"
  CARGO_TARGET_DIR="$PWD/src-tauri/target"
  case dev in
${dev}
  esac
  unset CARGO_TARGET_DIR
  case dev-prebuilt in
${prebuilt}
    ;;
  esac
  rm -f "$PREBUILT_LOG"
done
` });
      assert.equal(result.status, 0, result.stderr);
      const app = path.join(directory, 'src-tauri/target/named-dev', `debug/domios-app${exe}`);
      assert.equal(fs.readFileSync(app, 'utf8'), 'welcomeqa');
      const staged = platform === 'windows' ? path.join(path.dirname(app), 'domios-dev-app.exe') : path.join(path.dirname(app),'.domios-dev-runtime/bin/domios-dev-app');
      assert.equal(fs.readFileSync(staged, 'utf8'), 'welcomeqa');
      for (const instance of ['meshqa', 'welcomeqa']) {
        const recorded = fs.readFileSync(path.join(directory, `data-${instance}/built-app`), 'utf8').trim();
        const native = process.platform === 'win32' ? spawnSync(bash, ['-c', 'cygpath -w "$1"', '--', recorded], { encoding: 'utf8' }).stdout.trim() : recorded;
        assert.equal(path.normalize(native), staged);
        assert.equal(fs.existsSync(path.join(directory, `data-${instance}/target`)), false);
      }
      assert.equal(fs.existsSync(path.join(directory, 'src-tauri/target/dev-instances')), false);
      assert.deepEqual(fs.readFileSync(path.join(directory, 'refusal-checks'), 'utf8').trim().split('\n'), ['meshqa', 'welcomeqa']);
      assert.deepEqual(fs.readFileSync(path.join(directory, 'adoptions'), 'utf8').trim().split('\n'), ['dev:meshqa', 'dev:meshqa', 'dev:welcomeqa', 'dev:welcomeqa']);
      assert.equal(fs.readFileSync(defaultApp, 'utf8'), 'default running artifact');
    } finally {
      fs.rmSync(directory, { recursive: true, force: true });
    }
  });
}

for (const command of ['dev', 'dev-prebuilt', 'dev-server']) for (const live of [false, true]) {
  test(`inherited target child marker requires live reservation; command=${command}, live=${live}`, () => {
    const start = source.indexOf('case "$CMD" in\n  dev|dev-prebuilt|dev-server)\n');
    const entry = source.slice(start, source.indexOf('esac\n', start) + 5);
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-reservation-entry-'));
    try {
      const result = spawnSync(bash, ['-c', `set -euo pipefail
cd "$1"
CMD="$2"
export CODETERM_DEV_TARGET_CHILD=1
named_dev_isolated() { return 0; }
node() { echo lease-checked; [ "$1" = scripts/dev-launch-owner.mjs ]; [ "$2" = --validate-target-reservation ]; return ${live ? 0 : 1}; }
${entry}
echo admitted
`, '--', directory, command], { encoding: 'utf8' });
      assert.equal(result.status, live ? 0 : 2, result.stderr);
      assert.deepEqual(result.stdout.trim().split('\n'), live ? ['lease-checked', 'admitted'] : ['lease-checked']);
    } finally { fs.rmSync(directory, { recursive: true, force: true }); }
  });
}

for (const [command, instance, refused] of [['dev', 'alpha', true], ['dev', 'alpha', false], ['dev', '', false], ['dev-prebuilt', 'alpha', false]]) {
  test(`launch-owner entry names running DEVs before the instance lock refuses; ${command} instance=${instance || 'default'} refused=${refused}`, () => {
    const entry = source.slice(source.indexOf('case "$CMD" in\n  dev|dev-prebuilt)\n'), source.indexOf('esac\n', source.indexOf('case "$CMD" in\n  dev|dev-prebuilt)\n')) + 5);
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-launch-entry-'));
    let result;
    try {
      result = spawnSync(bash, ['-c', `set -euo pipefail
named_dev_isolated() { [ -n "$CT_INSTANCE" ]; }
refuse_named_dev_rebuild_while_running() { echo refusal-check; [ "$REFUSED" = false ]; }
CMD="$1" CT_INSTANCE="$2" REFUSED="$3" CT_DATA_DIR="$4"
printf '#!/bin/sh\\necho "launch-owner $1"\\n' > "$CT_DATA_DIR/node"
chmod +x "$CT_DATA_DIR/node"
PATH="$CT_DATA_DIR:$PATH"
${entry}
`, '--', command, instance, String(refused), directory], { encoding: 'utf8' });
    } finally { fs.rmSync(directory, { recursive: true, force: true }); }
    const checked = command === 'dev' && Boolean(instance);
    assert.equal(result.status, refused ? 2 : 0, result.stderr);
    assert.deepEqual(result.stdout.trim().split('\n'), [...(checked ? ['refusal-check'] : []),
      ...(refused ? [] : ['launch-owner scripts/dev-launch-owner.mjs'])]);
  });
}
