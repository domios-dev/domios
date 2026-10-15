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
  'tauri_debug_dir', 'dev_app_path', 'dev_cli_path'].map(name =>
  source.match(new RegExp(`^${name}\\(\\) \\{[\\s\\S]*?^\\}`, 'm'))[0]).join('\n');

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', ...options });
  assert.equal(result.status, 0, `${command}: ${result.error ?? ''}\n${result.stdout}\n${result.stderr}`);
  return result.stdout.trim();
}

function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-provenance-'));
  const checkout = path.join(directory, 'checkout');
  for (const dir of ['scripts', 'target/debug', 'data/bin', 'src-tauri/bundled-embed', 'dist']) {
    fs.mkdirSync(path.join(checkout, dir), { recursive: true });
  }
  for (const name of ['dev-prep.mjs', 'dev-app-process.mjs', 'windows-dev-app.ps1', 'windows-dev-app.cs', 'build-identity.cjs']) {
    fs.copyFileSync(path.join(root, 'scripts', name), path.join(checkout, 'scripts', name));
  }
  const exe = process.platform === 'win32' ? '.exe' : '';
  for (const file of [`target/debug/codeterm${exe}`, `data/bin/codeterm${exe}`,
    'src-tauri/bundled-embed/codeterm-embed-fixture', 'dist/index.html']) {
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
CT_DEV_EMBED_BUNDLE=src-tauri/bundled-embed/codeterm-embed-fixture
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

const preflight = source.slice(source.indexOf('  dev-prebuilt)'), source.indexOf('    if [ -n "$DEV_NOTIFY_TAB" ]; then\n      trap', source.indexOf('  dev-prebuilt)')));
for (const refusal of ['provenance', 'runtime', 'none']) {
  test(`prebuilt restart preflight refusal=${refusal} preserves stop ordering`, () => {
    const result = spawnSync(bash, ['-c', `set -euo pipefail
CT_INSTANCE= CARGO_TARGET_DIR= CT_SUFFIX= DEV_NOTIFY_TAB=
validate_dev_provenance() { echo provenance; ${refusal === 'provenance' ? 'return 1' : ':'}; }
preflight_dev_prebuilt_runtime() { echo runtime; ${refusal === 'runtime' ? 'return 1' : ':'}; }
kill_dev() { echo stopped; }
case dev-prebuilt in
${preflight}
;;
esac
`], { encoding: 'utf8' });
    assert.equal(result.status, refusal === 'none' ? 0 : 2, result.stderr);
    assert.equal(result.stdout.trim(), refusal === 'provenance' ? 'provenance' :
      refusal === 'runtime' ? 'provenance\nruntime' : 'provenance\nruntime\nstopped');
  });
}
