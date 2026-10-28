import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import test from 'node:test';

const root = path.resolve(import.meta.dirname, '../..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const source = readFileSync(path.join(root, 'build.sh'), 'utf8').replaceAll('\r\n', '\n');
const helpers = ['dev_build_phase', 'cli_profile_dir', 'cli_release_dir', 'cli_profile_binary', 'cli_release_binary', 'dev_cli_target_dir', 'build_dev_cli']
  .map(name => source.match(new RegExp(`^${name}\\(\\) \\{[^]*?^\\}`, 'm'))?.[0] || source.match(new RegExp(`^${name}\\(\\) \\{[^\\n]+`, 'm'))?.[0]).join('\n');
const start = source.indexOf('    echo "Building CLI..."');
const end = source.indexOf('\n    build_and_launch_dev "${@:2}"', start);
const body = source.slice(start, end);

for (const [platform, extension] of [['linux-gnu', ''], ['darwin', ''], ['msys', '.exe']]) {
  test(`CLI installation exposes byte-identical domios and codeterm commands: ${platform}`, () => {
    const directory = mkdtempSync(path.join(os.tmpdir(), 'cli alias install-'));
    try {
      const artifact = path.join(directory, 'artifact');
      writeFileSync(artifact, '#!/bin/sh\nprintf instance-cli', { mode: 0o755 });
      const installer = ['install_cli_to', 'cli_file_sha256', 'cli_file_sha256_or_status']
        .map(name => source.match(new RegExp(`^${name}\\(\\) \\{[^]*?^\\}`, 'm'))[0]).join('\n');
      const result = spawnSync(bash, ['-c', `set -euo pipefail
${installer}
OSTYPE="$2"
report_cli_install_failure() { printf '%s\\n' "$*" >&2; }
install_cli_to "$1/bin" "$1/artifact"
cmp "$1/bin/codeterm$3" "$1/bin/domios$3"
rm "$1/bin/domios$3"
ln "$1/bin/codeterm$3" "$1/bin/domios$3"
install_cli_to "$1/bin" "$1/artifact"
cmp "$1/bin/codeterm$3" "$1/bin/domios$3"
export PATH="$1/bin:$PATH"
command -v domios$3
`, '--', directory.replaceAll('\\', '/'), platform, extension], { encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      assert.ok(result.stdout.trim().endsWith(`/bin/domios${extension}`), result.stdout);
      assert.deepEqual(readFileSync(path.join(directory, `bin/domios${extension}`)), readFileSync(artifact));
    } finally { rmSync(directory, { recursive: true, force: true }); }
  });
}

for (const [platform, extension] of [['linux-gnu', ''], ['darwin', ''], ['msys', '.exe']]) {
  for (const target of ['', 'opaque-target']) {
    test(`DEV builds and installs its dev CLI artifact: ${platform}, target=${target || 'host'}`, () => {
      assert.ok(start > 0 && end > start);
      const logs = mkdtempSync(path.join(os.tmpdir(), 'dev-cli-profile-'));
      try {
      const result = spawnSync(bash, ['-c', `set -eu
        set -- dev
        ${helpers}
        OSTYPE=${platform}
        CARGO_TARGET_DIR='cache root'
        CARGO_BUILD_TARGET=${target}
        CT_DATA_DIR='dev data'
        tauri_debug_dir() { printf '%s' '${logs.replaceAll('\\', '/')}'; }
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
        'Building CLI...',
        `installed:dev data/bin:cache root/${target ? target + '/' : ''}debug/codeterm${extension}:opaque-hash`,
        `bundled:cache root/${target ? target + '/' : ''}debug/codeterm${extension}`,
        `production:${directory}release/codeterm${extension}`,
      ]);
      assert.equal(readFileSync(path.join(logs, '.domios-dev-build/cli.jsonl'), 'utf8').trim(), 'cargo build --manifest-path cli/Cargo.toml --message-format=json-render-diagnostics');
      } finally { rmSync(logs, { recursive: true, force: true }); }
    });
  }
}

test('a failed DEV CLI build stops before installing an artifact', () => {
  const logs = mkdtempSync(path.join(os.tmpdir(), 'dev-cli-failure-'));
  try {
  const result = spawnSync(bash, ['-c', `set -eu
    ${helpers}
    tauri_debug_dir() { printf '%s' '${logs.replaceAll('\\', '/')}'; }
    set -- dev
    cargo() { return 41; }
    install_cli_to() { printf unexpected-install; }
    ${body}
  `], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 41, result.stderr);
  assert.doesNotMatch(result.stdout, /unexpected-install/);
  } finally { rmSync(logs, { recursive: true, force: true }); }
});

for (const [platform, extension] of [['linux-gnu', ''], ['darwin', ''], ['msys', '.exe']]) {
  for (const target of ['', 'opaque-target']) {
    test(`named DEV preserves the exact CLI artifact after native compilation: ${platform}/${target || 'host'}`, () => {
      const directory = mkdtempSync(path.join(os.tmpdir(), 'dev cli collision-'));
      try {
        const layout = ['tauri_debug_dir', 'dev_built_app_path']
          .map(name => source.match(new RegExp(`^${name}\\(\\) \\{[^]*?^\\}`, 'm'))[0]).join('\n');
        const result = spawnSync(bash, ['-c', `set -euo pipefail
${helpers}
${layout}
cd "$1"
OSTYPE="$2" CARGO_BUILD_TARGET="$3"
app_target="$PWD/target/dev-instances/opaque-instance"
export CARGO_TARGET_DIR="$app_target"
cargo() {
  [ "$CARGO_TARGET_DIR" = "$app_target" ]
  artifact="$(cli_profile_binary debug)"
  mkdir -p "$(dirname "$artifact")"
  printf opaque-cli-bytes > "$artifact"
  printf 'cargo-receipt:%s\\n' "$artifact"
}
cli_file_sha256_or_status() { echo opaque-digest; }
build_dev_cli "$PWD/cli.jsonl"
[ "$CARGO_TARGET_DIR" = "$app_target" ]
native="$(dev_built_app_path)"
mkdir -p "$(dirname "$native")"
printf opaque-native-bytes > "$native"
[ "$native" != "$built_cli_artifact" ]
printf '%s\\n' "$built_cli_artifact" "$native" "$(cat "$built_cli_artifact")" "$(cat "$native")" "$(cat cli.jsonl)"
`, '--', directory, platform, target], {encoding: 'utf8'});
        assert.equal(result.status, 0, result.stderr);
        const [cli, native, cliBytes, nativeBytes, receipt] = result.stdout.trim().split(/\r?\n/);
        assert.ok(cli.endsWith(`/${target ? target + '/' : ''}debug/codeterm${extension}`));
        assert.ok(native.endsWith(`/${target ? target + '/' : ''}debug/domios-app${extension}`));
        assert.notEqual(cli, native);
        assert.equal(cliBytes, 'opaque-cli-bytes');
        assert.equal(nativeBytes, 'opaque-native-bytes');
        assert.equal(receipt, `cargo-receipt:${cli}`);
      } finally { rmSync(directory, {recursive: true, force: true}); }
    });
  }
}

for (const platform of ['linux-gnu', 'darwin', 'msys']) {
  for (const target of ['', 'opaque-target']) {
    test(`DEV layout retains raw Cargo outputs and derives staged paths: ${platform}/${target || 'host'}`, () => {
      const layout = ['tauri_debug_dir', 'dev_resource_dir', 'dev_built_app_path', 'dev_app_path']
        .map(name => source.match(new RegExp(`^${name}\\(\\) \\{[^]*?^\\}`, 'm'))[0]).join('\n');
      const result = spawnSync(bash, ['-c', `set -eu
${layout}
OSTYPE=${platform} CARGO_TARGET_DIR='cache root' CARGO_BUILD_TARGET=${target}
printf '%s\\n' "$(dev_built_app_path)" "$(dev_app_path)" "$(dev_resource_dir)"
`], { cwd: root, encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      const directory = `cache root/${target ? target + '/' : ''}debug`;
      assert.deepEqual(result.stdout.trim().split(/\r?\n/), platform === 'msys'
        ? [`${directory}/domios-app.exe`, `${directory}/domios-dev-app.exe`, directory]
        : [`${directory}/domios-app`, `${directory}/.domios-dev-runtime/bin/domios-dev-app`, `${directory}/.domios-dev-runtime/lib/codeterm`]);
    });
  }
}

for (const target of ['', 'aarch64-apple-darwin']) {
  test(`named macOS resource root is an app bundle beside its exact Cargo artifacts: ${target || 'host'}`, () => {
    const layout = ['tauri_debug_dir', 'dev_resource_dir', 'dev_built_app_path', 'dev_app_path']
      .map(name => source.match(new RegExp(`^${name}\\(\\) \\{[^]*?^\\}`, 'm'))[0]).join('\n');
    const result = spawnSync(bash, ['-c', `set -eu
${layout}
OSTYPE=darwin CARGO_TARGET_DIR='cache root/dev-instances/macdev-fix' CARGO_BUILD_TARGET="$1"
CODETERM_DEV_INSTANCE=macdev-fix
printf '%s\\n' "$(dev_built_app_path)" "$(dev_app_path)" "$(dev_resource_dir)"
`, '--', target], { cwd: root, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const debug = `cache root/dev-instances/macdev-fix/${target ? target + '/' : ''}debug`;
    assert.deepEqual(result.stdout.trim().split(/\r?\n/), [
      `${debug}/domios-app`,
      `${debug}/.domios-dev-runtime/Domios.app/Contents/MacOS/domios-dev-app`,
      `${debug}/.domios-dev-runtime/Domios.app/Contents/Resources`,
    ]);
  });
}
