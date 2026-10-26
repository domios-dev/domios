import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const source = fs.readFileSync(new URL('../../build.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const helpers = ['named_dev_target_dir', 'named_dev_isolated', 'prepare_named_dev_target', 'dev_cli_target_dir', 'build_dev_cli',
  'tauri_debug_dir', 'dev_built_app_path', 'dev_app_path'].map(name => source.match(new RegExp(`^${name}\\(\\) \\{[\\s\\S]*?^\\}`, 'm'))[0]).join('\n');
const dev = source.slice(source.indexOf('  dev)\n'), source.indexOf('  e2e)\n'));
const prebuilt = source.slice(source.indexOf('  dev-prebuilt)\n'), source.indexOf('    if [ -n "$DEV_NOTIFY_TAB" ]; then\n      trap', source.indexOf('  dev-prebuilt)\n')));

for (const platform of ['windows','linux']) for (const wait of [0, 1]) {
  test(`named DEV build and standalone prebuilt use checkout artifacts; platform=${platform}, wait=${wait}`, () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-named-target-'));
    try {
      const exe = platform === 'windows' ? '.exe' : '';
      const defaultApp = path.join(directory, `src-tauri/target/debug/codeterm${exe}`);
      fs.mkdirSync(path.dirname(defaultApp), { recursive: true });
      fs.writeFileSync(defaultApp, 'default running artifact');
      const result = spawnSync(bash, ['-c', `set -euo pipefail
${helpers}
cd "$1"
OSTYPE="$3"
CMD=dev CT_DEV_WAIT="$2" CT_PORT=7799 DEV_NOTIFY_TAB= CT_SUFFIX="-target-test-$$"
CARGO_BUILD_TARGET= CODETERM_BUILD_SHA=opaque
dev_health_status() { :; }
dev_health_fields() { :; }
ct_verify_target_dir() { :; }
kill_dev() { :; }
stop_dev_before_replacement() { :; }
enter_cargo_gateway() { :; }
prepare_dev_frontend_dist() { :; }
ensure_deps() { :; }
cargo() { [ "$CARGO_TARGET_DIR" = "$PWD/src-tauri/target/dev-instances/$CT_INSTANCE_KEY" ]; }
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
`, '--', directory, String(wait), platform === 'windows'?'msys':'linux-gnu'], { encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      for (const instance of ['meshqa', 'welcomeqa']) {
        const app = path.join(directory, 'src-tauri/target/dev-instances', instance, `debug/codeterm${exe}`);
        assert.equal(fs.readFileSync(app, 'utf8'), instance);
        const staged = platform === 'windows' ? path.join(path.dirname(app), 'domios-dev-app.exe') : path.join(path.dirname(app),'.domios-dev-runtime/bin/domios-dev-app');
        assert.equal(fs.readFileSync(staged, 'utf8'), instance);
        assert.equal(fs.existsSync(path.join(directory, `data-${instance}/target`)), false);
      }
      assert.equal(fs.readFileSync(defaultApp, 'utf8'), 'default running artifact');
    } finally {
      fs.rmSync(directory, { recursive: true, force: true });
    }
  });
}
