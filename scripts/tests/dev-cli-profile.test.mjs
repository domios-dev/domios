import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import test from 'node:test';

const root = path.resolve(import.meta.dirname, '../..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const source = readFileSync(path.join(root, 'build.sh'), 'utf8').replaceAll('\r\n', '\n');
const helpers = ['cli_profile_dir', 'cli_release_dir', 'cli_profile_binary', 'cli_release_binary']
  .map(name => source.match(new RegExp(`^${name}\\(\\) \\{[^]*?^\\}`, 'm'))?.[0] || source.match(new RegExp(`^${name}\\(\\) \\{[^\\n]+`, 'm'))?.[0]).join('\n');
const start = source.indexOf('    echo "Building CLI..."');
const end = source.indexOf('\n    build_and_launch_dev "${@:2}"', start);
const body = source.slice(start, end);

for (const [platform, extension] of [['linux-gnu', ''], ['darwin', ''], ['msys', '.exe']]) {
  for (const target of ['', 'opaque-target']) {
    test(`DEV builds and installs its dev CLI artifact: ${platform}, target=${target || 'host'}`, () => {
      assert.ok(start > 0 && end > start);
      const result = spawnSync(bash, ['-c', `set -eu
        set -- dev
        ${helpers}
        OSTYPE=${platform}
        CARGO_TARGET_DIR='cache root'
        CARGO_BUILD_TARGET=${target}
        CT_DATA_DIR='dev data'
        cargo() { printf 'cargo %s\\n' "$*"; }
        cli_file_sha256_or_status() { printf opaque-hash; }
        install_cli_to() { printf 'installed:%s:%s:%s\\n' "$1" "$2" "$3"; }
        ${body}
        printf 'bundled:%s\\n' "$built_cli_artifact"
        printf 'production:%s\\n' "$(cli_release_binary)"
      `], { cwd: root, encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      const directory = `cache root/${target ? target + '/' : ''}`;
      assert.deepEqual(result.stdout.trim().split(/\r?\n/), [
        'Building CLI...', 'cargo build --manifest-path cli/Cargo.toml',
        `installed:dev data/bin:${directory}debug/codeterm${extension}:opaque-hash`,
        `bundled:${directory}debug/codeterm${extension}`,
        `production:${directory}release/codeterm${extension}`,
      ]);
    });
  }
}

test('a failed DEV CLI build stops before installing an artifact', () => {
  const result = spawnSync(bash, ['-c', `set -eu
    set -- dev
    cargo() { return 41; }
    install_cli_to() { printf unexpected-install; }
    ${body}
  `], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 41, result.stderr);
  assert.doesNotMatch(result.stdout, /unexpected-install/);
});
