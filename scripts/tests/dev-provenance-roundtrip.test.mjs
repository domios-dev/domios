import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const root = path.resolve(import.meta.dirname, '../..');
const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8').replaceAll('\r\n', '\n');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const functions = ['dev_provenance_record', 'write_dev_provenance', 'validate_dev_provenance',
  'tauri_debug_dir', 'dev_resource_dir', 'dev_frontend_dir', 'dev_built_app_path', 'dev_app_path', 'dev_cli_path', 'named_dev_target_dir', 'named_dev_isolated'].map(sourceFunction).join('\n');

const runtimeDebug = process.platform === 'win32' ? 'target/debug' : 'target/debug/.domios-dev-runtime';
const resourceDebug = process.platform === 'win32' ? runtimeDebug : `${runtimeDebug}/lib/codeterm`;
const clientDebug = process.platform === 'win32' ? runtimeDebug : `${runtimeDebug}/bin`;

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', ...options });
  assert.equal(result.status, 0, `${command}: ${result.error ?? ''}\n${result.stdout}\n${result.stderr}`);
  return result.stdout.trim();
}

function splitResources(directory, mac = false) {
  const exe = process.platform === 'win32' ? '.exe' : '';
  for (const name of [mac ? '../MacOS/domios-server' : `domios-server/domios-server${exe}`, `cli/codeterm${exe}`, 'server-component.json',
    'dist/index.html', 'skills/fixture.md', 'LICENSE', 'NOTICE', ...(process.platform === 'win32' ? ['conpty/x64/conpty.dll'] : [])]) {
    const file = path.join(directory, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, `opaque split artifact: ${name}`, { mode: 0o755 });
  }
}

function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-provenance-'));
  const checkout = path.join(directory, 'checkout');
  for (const dir of ['scripts', clientDebug, 'data/bin', 'dist']) {
    fs.mkdirSync(path.join(checkout, dir), { recursive: true });
  }
  for (const name of ['dev-prep.mjs', 'server-readiness.mjs', 'dev-native-layout.mjs', 'dev-reload-inputs.cjs', 'crate-layers.json', 'dev-app-process.mjs', 'dev-runtime-environment.mjs', 'windows-dev-app.ps1', 'windows-dev-app.cs', 'windows-dev-desktop.cs', 'build-identity.cjs']) {
    fs.copyFileSync(path.join(root, 'scripts', name), path.join(checkout, 'scripts', name));
  }
  const exe = process.platform === 'win32' ? '.exe' : '';
  for (const file of [`${clientDebug}/domios-dev-app${exe}`, `data/bin/codeterm${exe}`,
    'dist/index.html']) {
    fs.writeFileSync(path.join(checkout, file), `opaque artifact: ${file}`, { mode: 0o755 });
  }
  fs.writeFileSync(path.join(checkout, 'package.json'), '{"private":true}');
  splitResources(path.join(checkout, resourceDebug));
  run('git', ['init', '-q'], { cwd: checkout });
  run('git', ['add', '.'], { cwd: checkout });
  run('git', ['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
    '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'fixture'], { cwd: checkout });
  const sha = run('git', ['rev-parse', 'HEAD'], { cwd: checkout });
  return { directory, checkout, sha };
}

function provenance(checkout, cwd, target, sha, operation, launchSha = '', instance = '') {
  return spawnSync(bash, ['-c', `set -eo pipefail
${functions}
cd "$1"
CT_DATA_DIR="$2/data"
CARGO_TARGET_DIR="$3/target"
CARGO_BUILD_TARGET=
CT_INSTANCE=fixture CT_PORT=7786 CT_VITE_PORT=1520 CT_CDP_PORT=9323 CT_IDENTIFIER=fixture.dev
if [ -n "$6" ]; then
  CT_INSTANCE="$6" CT_INSTANCE_KEY="$6"
  CT_DATA_DIR="$2/data/$6"
  export CODETERM_DEV_INSTANCE="$6"
  CARGO_TARGET_DIR="$(named_dev_target_dir)"
fi
CODETERM_BUILD_SHA="$4"
CT_DEV_PROVENANCE_SHA="$5"
${operation} "$4"
`, '--', cwd, checkout, target, sha, launchSha, instance], { encoding: 'utf8' });
}

test('two named instances persist and validate their own checkout artifact provenance', () => {
  const { directory, checkout, sha } = fixture();
  try {
    fs.writeFileSync(path.join(checkout, '.gitignore'), 'src-tauri/target/\n');
    const exe = process.platform === 'win32' ? '.exe' : '';
    const records = [];
    for (const instance of ['meshqa', 'welcomeqa']) {
      const app = path.join(checkout, 'src-tauri/target/dev-instances', instance, process.platform === 'win32' ? `debug/domios-dev-app${exe}` : `debug/.domios-dev-runtime/${process.platform === 'darwin' ? 'Domios.app/Contents/MacOS' : 'bin'}/domios-dev-app${exe}`);
      const cli = path.join(checkout, 'data', instance, `bin/codeterm${exe}`);
      for (const file of [app, cli]) {
        fs.mkdirSync(path.dirname(file), { recursive: true });
        fs.writeFileSync(file, instance, { mode: 0o755 });
      }
      splitResources(process.platform === 'win32' ? path.dirname(app) : path.resolve(path.dirname(app), process.platform === 'darwin' ? '../Resources' : '../lib/codeterm'), process.platform === 'darwin');
      const written = provenance(checkout, checkout, checkout, sha, 'write_dev_provenance', '', instance);
      assert.equal(written.status, 0, written.stderr);
      const checked = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance', '', instance);
      assert.equal(checked.status, 0, checked.stderr);
      const record = JSON.parse(fs.readFileSync(path.join(checkout, 'data', instance, 'dev-build-provenance.json')));
      assert.equal(record.instance.name, instance);
      assert.equal(record.artifacts.app.path, fs.realpathSync.native(app));
      records.push(record);
    }
    assert.notEqual(records[0].artifacts.app.path, records[1].artifacts.app.path);
    fs.appendFileSync(records[1].artifacts.app.path, 'changed');
    const unaffected = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance', '', 'meshqa');
    assert.equal(unaffected.status, 0, unaffected.stderr);
    const changed = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance', '', 'welcomeqa');
    assert.equal(changed.status, 1);
    assert.match(changed.stderr, /app artifact content mismatch/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('utility identity capture tolerates non-Git directories and unavailable Git; stamping refuses', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-identity-no-git-'));
  try {
    for (const missingGit of [false, true]) {
      const result = spawnSync(bash, ['-c', `set -euo pipefail
${sourceFunction('initialize_build_identity')}
${missingGit ? 'git() { return 127; }' : ''}
for CMD in env help kill; do
  export CODETERM_BUILD_SHA=stale CODETERM_BUILD_HASH=stale CODETERM_FRONTEND_BUILD_HASH=stale
  initialize_build_identity
  [ -z "\${CODETERM_BUILD_SHA+x}" ]
  [ -z "\${CODETERM_BUILD_HASH+x}" ]
  [ -z "\${CODETERM_FRONTEND_BUILD_HASH+x}" ]
done
for CMD in dev dev-prebuilt build install-local; do
  if initialize_build_identity; then exit 42; fi
done
`, '--'], { cwd: directory, encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
    }
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('commit during build preserves launch identity across prebuilt handoff', () => {
  const { directory, checkout, sha } = fixture();
  try {
    const initialized = spawnSync(bash, ['-c', `set -euo pipefail
${sourceFunction('initialize_build_identity')}
CMD=dev
initialize_build_identity
original_short="$CODETERM_BUILD_HASH"
git -c user.name=Fixture -c user.email=fixture@example.invalid -c core.hooksPath=/dev/null commit --allow-empty -qm later
CT_DEV_HANDOFF_SHA="$CODETERM_BUILD_SHA"
CT_DEV_HANDOFF_HASH="$CODETERM_BUILD_HASH"
export CT_DEV_HANDOFF_SHA CT_DEV_HANDOFF_HASH
export -f initialize_build_identity
bash -c '
set -euo pipefail
CMD=dev-prebuilt
initialize_build_identity
[ "$CODETERM_BUILD_SHA" = "$1" ]
[ "$CODETERM_BUILD_HASH" = "$2" ]
[ "$CODETERM_FRONTEND_BUILD_HASH" = "$2" ]
[ "$CT_DEV_PROVENANCE_SHA" = "$1" ]
[ -z "\${CT_DEV_HANDOFF_SHA+x}" ]
[ -z "\${CT_DEV_HANDOFF_HASH+x}" ]
' -- "$1" "$original_short"
node -e 'const {resolveBuildIdentity}=require("./scripts/build-identity.cjs"); if(resolveBuildIdentity()!==process.env.CODETERM_BUILD_HASH) process.exit(1)'
`, '--', sha], { cwd: checkout, encoding: 'utf8' });
    assert.equal(initialized.status, 0, initialized.stderr);
    assert.notEqual(run('git', ['rev-parse', 'HEAD'], { cwd: checkout }), sha);
    const written = provenance(checkout, checkout, checkout, sha, 'write_dev_provenance');
    assert.equal(written.status, 0, written.stderr);
    const internal = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance', sha);
    assert.equal(internal.status, 0, internal.stderr);
    const standalone = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance');
    assert.equal(standalone.status, 1);
    assert.match(standalone.stderr, /recorded build commit does not match/);
    fs.appendFileSync(path.join(checkout, resourceDebug, 'dist/index.html'), 'changed');
    const tampered = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance', sha);
    assert.equal(tampered.status, 1);
    assert.match(tampered.stderr, /frontend artifact content mismatch/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

for (const aliasKind of process.platform === 'win32' ? ['junction', 'subst'] : ['symlink']) {
  test(`gateway write then standalone prebuilt verifies after ${aliasKind} removal`, () => {
    const { directory, checkout, sha } = fixture();
    let alias;
    let release;
    try {
      if (aliasKind === 'subst') {
        const drive = [...'WVUTSRQPONMLKJIHGFED'].find(letter => !fs.existsSync(`${letter}:/`));
        assert.ok(drive, 'fixture needs an unused drive letter');
        run('subst', [`${drive}:`, checkout]);
        alias = `${drive}:/`;
        release = () => run('subst', [`${drive}:`, '/D']);
      } else {
        alias = path.join(directory, 'alias');
        fs.symlinkSync(checkout, alias, process.platform === 'win32' ? 'junction' : 'dir');
        release = () => fs.unlinkSync(alias);
      }
      const written = provenance(checkout, alias, alias, sha, 'write_dev_provenance');
      assert.equal(written.status, 0, written.stderr);
      const record = JSON.parse(fs.readFileSync(path.join(checkout, 'data/dev-build-provenance.json')));
      for (const artifact of Object.values(record.artifacts)) {
        assert.ok(artifact.path.startsWith(fs.realpathSync.native(checkout) + path.sep), artifact.path);
      }
      release();
      release = undefined;
      const checked = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance');
      assert.equal(checked.status, 0, checked.stderr);
      assert.match(checked.stdout, /\[dev-provenance\] matched/);
      const app = record.artifacts.app.path;
      fs.appendFileSync(app, 'changed');
      const changed = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance');
      assert.equal(changed.status, 1);
      assert.match(changed.stderr, /app artifact content mismatch/);
    } finally {
      if (release) release();
      fs.rmSync(directory, { recursive: true, force: true });
    }
  });
}

function sourceFunction(name) {
  return source.match(new RegExp(`^${name}\\(\\) \\{[\\s\\S]*?^\\}`, 'm'))[0];
}

test('frontend rewritten with identical bytes is accepted; different bytes still refuse', () => {
  const { directory, checkout, sha } = fixture();
  try {
    const dist = path.join(checkout, resourceDebug, 'dist');
    const written = provenance(checkout, checkout, checkout, sha, 'write_dev_provenance');
    assert.equal(written.status, 0, written.stderr);
    const original = fs.readFileSync(path.join(dist, 'index.html'));
    fs.rmSync(dist, { recursive: true });
    fs.mkdirSync(dist);
    fs.writeFileSync(path.join(dist, 'index.html'), original);
    const same = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance');
    assert.equal(same.status, 0, same.stderr);
    fs.writeFileSync(path.join(dist, 'index.html'), Buffer.concat([original, Buffer.from('rebuilt')]));
    const changed = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance');
    assert.equal(changed.status, 1);
    assert.match(changed.stderr, /frontend artifact content mismatch/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('prebuilt web watcher rebuilds dist in place instead of emptying it under the static server', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-web-watch-'));
  try {
    const result = spawnSync(bash, ['-c', `set -euo pipefail
${sourceFunction('start_web_watch')}
${sourceFunction('dev_frontend_dir')}
cd "$1"
CT_SUFFIX="-watch-test-$$"
kill_web_watch() { :; }
dev_resource_dir() { echo target/debug/resources; }
node() { printf '%s|%s\\n' "$CODETERM_FRONTEND_DEV" "$*" > args.txt; }
nohup() { "$@"; }
process_pid_for_cleanup() { echo "$1"; }
web_watch_pidfile() { echo pid.txt; }
start_web_watch > /dev/null
wait
rm -f "/tmp/codeterm-web-watch$CT_SUFFIX.log"
cat args.txt
`, '--', directory], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const [frontendDev, args] = result.stdout.trim().split('|');
    assert.equal(frontendDev, '1');
    assert.match(args, /build --watch\b/);
    assert.match(args, /--emptyOutDir false/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

const preflight = source.slice(source.indexOf('  dev-prebuilt)'), source.indexOf('    if [ -n "$DEV_NOTIFY_TAB" ]; then\n      trap', source.indexOf('  dev-prebuilt)')));
const preflightHelpers = ['run_prebuilt_preflight', 'refuse_prebuilt'].map(sourceFunction).join('\n');
for (const externalFrontend of [0, 1]) {
  test(`prebuilt timeout shell preserves ready server and window only in split mode=${externalFrontend}`, () => {
    const start = source.indexOf('      if run_outside_tab_job node scripts/dev-prep.mjs --launch-prebuilt');
    const launch = source.slice(start, source.indexOf('      HEALTH=', start));
    const result = spawnSync(bash, ['-c', `set -euo pipefail
${sourceFunction('cleanup_failed_prebuilt_launch')}
DEV_FRONT_EXTERNAL=${externalFrontend} PREBUILT_LOG=fixture.log CT_PORT=17886
kill_dev() { echo killed; }
dev_app_path() { echo fixture-app; }
dev_provenance_record() { echo fixture-record; }
run_outside_tab_job() { return 3; }
trap cleanup_failed_prebuilt_launch EXIT
${launch}
echo falsely-ready
`], { encoding: 'utf8' });
    assert.equal(result.status, 3, result.stderr);
    assert.equal(result.stdout.trim(), externalFrontend ? '' : 'killed');
    assert.match(result.stderr, externalFrontend ? /still attaching; server and window retained/ : /cleaning the owned dev processes/);
  });
}

for (const refusal of ['provenance', 'runtime', 'none']) {
  test(`prebuilt restart preflight refusal=${refusal} preserves stop ordering and logs refusals`, () => {
    const result = spawnSync(bash, ['-c', `set -euo pipefail
${preflightHelpers}
CT_INSTANCE= CARGO_TARGET_DIR= CT_SUFFIX="-provenance-test-$$" DEV_NOTIFY_TAB=
validate_dev_provenance() { ${refusal === 'provenance' ? 'echo "[dev-prep] frontend artifact content mismatch" >&2; return 1' : 'echo provenance'}; }
preflight_dev_prebuilt_runtime() { ${refusal === 'runtime' ? 'echo "ERROR: prebuilt dev cannot execute pnpm" >&2; return 1' : 'echo runtime'}; }
kill_dev() { echo stopped; }
set +e
(
case dev-prebuilt in
${preflight}
;;
esac
)
status=$?
echo "--- log"
cat "/tmp/codeterm-dev-prebuilt$CT_SUFFIX.log" 2>/dev/null
rm -f "/tmp/codeterm-dev-prebuilt$CT_SUFFIX.log"
exit "$status"
`], { encoding: 'utf8' });
    assert.equal(result.status, refusal === 'none' ? 0 : 2, result.stderr);
    const [stdout, log] = result.stdout.split('--- log\n');
    assert.equal(stdout.trim(), refusal === 'provenance' ? '' :
      refusal === 'runtime' ? 'provenance' : 'provenance\nruntime\nstopped');
    if (refusal === 'provenance') {
      assert.match(log, /\[dev-prep\] frontend artifact content mismatch\nERROR: prebuilt dev provenance/);
    } else if (refusal === 'runtime') {
      assert.match(log, /cannot execute pnpm\nERROR: prebuilt dev runtime prerequisites/);
    }
    if (refusal !== 'none') assert.match(result.stderr, /refusing before stopping dev/);
  });
}

for (const [name, relative, message] of [
  ['server', `domios-server/domios-server${process.platform === 'win32' ? '.exe' : ''}`, 'server artifact content mismatch'],
  ['inventory', 'server-component.json', 'inventory artifact content mismatch'],
  ['runtime resource', 'skills/fixture.md', 'resources artifact content mismatch'],
]) {
  test(`prebuilt refuses changed ${name} bytes`, () => {
    const { directory, checkout, sha } = fixture();
    try {
      assert.equal(provenance(checkout, checkout, checkout, sha, 'write_dev_provenance').status, 0);
      fs.appendFileSync(path.join(checkout, resourceDebug, relative), 'changed');
      const result = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance');
      assert.equal(result.status, 1);
      assert.ok(result.stderr.includes(message), result.stderr);
    } finally { fs.rmSync(directory, { recursive: true, force: true }); }
  });
}


test('prebuilt refuses changed desktop execution helper source', () => {
  const { directory, checkout, sha } = fixture();
  try {
    const written = provenance(checkout, checkout, checkout, sha, 'write_dev_provenance');
    assert.equal(written.status, 0, written.stderr);
    fs.appendFileSync(path.join(checkout, 'scripts/windows-dev-desktop.cs'), '\n');
    const checked = provenance(checkout, checkout, checkout, sha, 'validate_dev_provenance');
    assert.equal(checked.status, 1);
    assert.match(checked.stderr, /source|input|dirty/);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('server reload classifies current bytes and keeps artifact/instance validation', () => {
  const { directory, checkout, sha } = fixture();
  const server = path.join(checkout, 'src-tauri/crates/domios-server/src/main.rs');
  const client = path.join(checkout, 'src-tauri/src/host_ports.rs');
  try {
    for (const file of [server, client]) {
      fs.mkdirSync(path.dirname(file), {recursive: true});
      fs.writeFileSync(file, 'dirty at original build');
    }
    assert.equal(provenance(checkout, checkout, checkout, sha, 'write_dev_provenance').status, 0);
    fs.appendFileSync(server, 'new server input');
    const reload = () => provenance(checkout, checkout, checkout, sha,
      'CT_DEV_PROVENANCE_OPERATION=--check-server-reload validate_dev_provenance');
    let result = reload();
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /^server:[a-f0-9]{64}/);
    fs.appendFileSync(client, 'new client input');
    result = reload();
    assert.equal(result.status, 1);
    assert.match(result.stderr, /run \.\/build.sh dev instead/);
    fs.writeFileSync(client, 'dirty at original build');
    fs.appendFileSync(path.join(checkout, resourceDebug, 'server-component.json'), 'tampered');
    result = reload();
    assert.equal(result.status, 1);
    assert.match(result.stderr, /inventory artifact content mismatch/);
  } finally { fs.rmSync(directory, {recursive: true, force: true}); }
});
