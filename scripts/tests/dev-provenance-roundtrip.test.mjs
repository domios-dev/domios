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
  'tauri_debug_dir', 'dev_app_path', 'dev_cli_path'].map(sourceFunction).join('\n');

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', ...options });
  assert.equal(result.status, 0, `${command}: ${result.error ?? ''}\n${result.stdout}\n${result.stderr}`);
  return result.stdout.trim();
}

function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-provenance-'));
  const checkout = path.join(directory, 'checkout');
  for (const dir of ['scripts', 'target/debug', 'data/bin', 'dist']) {
    fs.mkdirSync(path.join(checkout, dir), { recursive: true });
  }
  for (const name of ['dev-prep.mjs', 'dev-app-process.mjs', 'dev-runtime-environment.mjs', 'windows-dev-app.ps1', 'windows-dev-app.cs', 'build-identity.cjs']) {
    fs.copyFileSync(path.join(root, 'scripts', name), path.join(checkout, 'scripts', name));
  }
  const exe = process.platform === 'win32' ? '.exe' : '';
  for (const file of [`target/debug/codeterm${exe}`, `data/bin/codeterm${exe}`,
    'dist/index.html']) {
    fs.writeFileSync(path.join(checkout, file), `opaque artifact: ${file}`, { mode: 0o755 });
  }
  fs.writeFileSync(path.join(checkout, 'package.json'), '{"private":true}');
  run('git', ['init', '-q'], { cwd: checkout });
  run('git', ['add', '.'], { cwd: checkout });
  run('git', ['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
    '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'fixture'], { cwd: checkout });
  const sha = run('git', ['rev-parse', 'HEAD'], { cwd: checkout });
  return { directory, checkout, sha };
}

function provenance(checkout, cwd, target, sha, operation) {
  return spawnSync(bash, ['-c', `set -euo pipefail
${functions}
cd "$1"
CT_DATA_DIR="$2/data"
CARGO_TARGET_DIR="$3/target"
CARGO_BUILD_TARGET=
CT_INSTANCE=fixture CT_PORT=7786 CT_VITE_PORT=1520 CT_CDP_PORT=9323 CT_IDENTIFIER=fixture.dev
${operation} "$4"
`, '--', cwd, checkout, target, sha], { encoding: 'utf8' });
}

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
    const dist = path.join(checkout, 'dist');
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

test('dev rebuilds the frontend for HEAD before recording provenance', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-frontend-order-'));
  try {
    fs.writeFileSync(path.join(directory, 'build.sh'), 'echo handoff\n');
    fs.writeFileSync(path.join(directory, 'conf.json'), '{}');
    const result = spawnSync(bash, ['-c', `set -euo pipefail
${sourceFunction('build_and_launch_dev')}
cd "$1"
configure_dev_cargo() { :; }
dev_conf_path() { echo conf.json; }
node() { :; }
cargo() { echo cargo; }
git() { echo "$FIXTURE_SHA"; }
build_dev_frontend() { echo frontend; }
write_dev_provenance() { echo "provenance $1"; }
retain_dev_pdb() { :; }
release_stable_build_path() { :; }
tauri_debug_dir() { :; }
record_dev_target() { :; }
complete_build_gateway_before_handoff() { :; }
CT_DEV_CARGO_PROFILE=dev CT_DEV_CARGO_FEATURES=
build_and_launch_dev
`, '--', directory], { encoding: 'utf8', env: { ...process.env, FIXTURE_SHA: 'a'.repeat(40) } });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim().split('\n').filter(line => !line.startsWith('Build gateway')).join('\n'),
      `cargo\nfrontend\nprovenance ${'a'.repeat(40)}\nhandoff`);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('prebuilt web watcher rebuilds dist in place instead of emptying it under the static server', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-web-watch-'));
  try {
    const result = spawnSync(bash, ['-c', `set -euo pipefail
${sourceFunction('start_web_watch')}
cd "$1"
CT_SUFFIX="-watch-test-$$"
kill_web_watch() { :; }
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
