// Caller: build.sh cache-wrapper-check
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
import { cargoBuildDirectory, prepareCacheWrapper } from '../cache-rustc-wrapper.mjs';

test('identical compiler wrappers share identity across checkout paths; changed source stays isolated', {
  skip: process.env.CODETERM_CACHE_WRAPPER_TEST !== '1',
  timeout: 120000,
}, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cache-wrapper-identity-'));
  const env = { ...process.env, CARGO_HOME: path.join(root, 'cargo') };
  try {
    const modules = [];
    for (const name of ['first', 'second', 'changed']) {
      const directory = path.join(root, name);
      fs.mkdirSync(directory);
      for (const file of ['cache-rustc-wrapper.mjs', 'cache-rustc-wrapper.rs']) {
        fs.copyFileSync(new URL(`../${file}`, import.meta.url), path.join(directory, file));
      }
      if (name === 'second') {
        const source = path.join(directory, 'cache-rustc-wrapper.rs');
        fs.writeFileSync(source, fs.readFileSync(source, 'utf8').replaceAll('\r\n', '\n').replaceAll('\n', '\r\n'));
      }
      if (name === 'changed') fs.appendFileSync(path.join(directory, 'cache-rustc-wrapper.rs'), '\n');
      modules.push(await import(pathToFileURL(path.join(directory, 'cache-rustc-wrapper.mjs')).href));
    }
    const first = modules[0].prepareCacheWrapper({ env });
    const compiler = fs.readFileSync(first.replace(/(?:\.exe)?$/, '.compiler'), 'utf8');
    assert.match(execFileSync(first, ['--version'], { env, encoding: 'utf8' }), /^sccache /);
    assert.match(execFileSync(first, [compiler, '-vV'], { env, encoding: 'utf8' }), /^rustc /);
    const timestamp = fs.statSync(first).mtimeMs;
    assert.equal(modules[1].prepareCacheWrapper({ env }), first);
    assert.equal(fs.statSync(first).mtimeMs, timestamp);
    assert.notEqual(modules[2].prepareCacheWrapper({ env }), first);
    const source = path.join(root, 'tracked.rs');
    fs.writeFileSync(source, 'pub fn directory() -> &\'static str { env!("CARGO_BUILD_BUILD_DIR") }\n');
    const consumer = path.join(root, 'consumer.rs');
    fs.writeFileSync(consumer, 'fn main() { println!("{}", tracked::directory()); }\n');
    const artifacts = [];
    for (const name of ['build-a', 'build-b']) {
      const directory = path.join(root, name);
      fs.mkdirSync(directory);
      execFileSync(first, [compiler, '--crate-name', 'tracked', '--crate-type', 'rlib',
        '--emit=link,dep-info', '--out-dir', directory, source], {
        env: { ...env, CARGO_BUILD_BUILD_DIR: directory, CARGO_INCREMENTAL: '0' },
      });
      const executable = path.join(directory, process.platform === 'win32' ? 'consumer.exe' : 'consumer');
      execFileSync(compiler, ['--edition=2021', consumer, '--extern',
        `tracked=${path.join(directory, 'libtracked.rlib')}`, '-o', executable], { env });
      assert.equal(execFileSync(executable, [], { encoding: 'utf8' }).trim(), directory);
      artifacts.push(fs.readFileSync(path.join(directory, 'libtracked.rlib')));
    }
    assert.ok(!artifacts[0].equals(artifacts[1]), 'tracked build directories must retain their actual values');
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('configured build directories produce equal generated metadata and downstream Rust cache hits', {
  skip: process.env.CODETERM_CACHE_WRAPPER_TEST !== '1',
  timeout: 120000,
}, () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cache-configured-build-'));
  const env = { ...process.env, CARGO_HOME: path.join(root, 'cargo'), CARGO_INCREMENTAL: '0' };
  delete env.CARGO_BUILD_BUILD_DIR;
  try {
    fs.mkdirSync(path.join(root, '.cargo'));
    fs.mkdirSync(path.join(root, 'src'));
    fs.copyFileSync(new URL('../../rust-toolchain.toml', import.meta.url), path.join(root, 'rust-toolchain.toml'));
    fs.writeFileSync(path.join(root, 'Cargo.toml'), '[package]\nname="cache-configured-fixture"\nversion="0.0.0"\nedition="2021"\n[workspace]\n');
    fs.writeFileSync(path.join(root, 'src/lib.rs'), 'pub fn value() {}\n');
    const source = path.join(root, 'generated.rs');
    fs.writeFileSync(source, 'include!(concat!(env!("OUT_DIR"), "/private.rs"));\npub const EXPLICIT: Option<&str> = option_env!("CARGO_BUILD_BUILD_DIR");\n');
    const consumer = path.join(root, 'consumer.rs');
    fs.writeFileSync(consumer, 'pub fn value() -> generated_fixture::Generated { generated_fixture::Generated }\n');
    const wrapper = prepareCacheWrapper({ env });
    const compiler = fs.readFileSync(wrapper.replace(/(?:\.exe)?$/, '.compiler'), 'utf8');
    const hits = () => JSON.parse(execFileSync('sccache', ['--show-stats', '--stats-format', 'json'], { env, encoding: 'utf8' })).stats.cache_hits.counts.Rust || 0;
    const metadata = [];
    for (const name of ['a', 'b']) {
      const build = path.join(root, `build-${name}`);
      fs.writeFileSync(path.join(root, '.cargo/config.toml'), `[build]\nbuild-dir=${JSON.stringify(build)}\n`);
      const resolved = cargoBuildDirectory('Cargo.toml', { env, cwd: root });
      assert.equal(path.resolve(resolved), path.resolve(build));
      assert.equal(env.CARGO_BUILD_BUILD_DIR, undefined);
      const output = path.join(build, 'debug/build/generated-opaque/out');
      const deps = path.join(build, 'debug/deps');
      fs.mkdirSync(output, { recursive: true });
      fs.mkdirSync(deps, { recursive: true });
      fs.writeFileSync(path.join(output, 'private.rs'), 'pub struct Generated;\n');
      const settings = { ...env, CARGO_TARGET_DIR: path.join(root, `final-${name}`), CODETERM_CACHE_BUILD_DIRECTORY: resolved, OUT_DIR: output };
      execFileSync(wrapper, [compiler, '--crate-name', 'generated_fixture', '--crate-type', 'rlib',
        '--edition=2021', '--emit=link,metadata,dep-info', '--out-dir', deps, source], { env: settings });
      metadata.push(fs.readFileSync(path.join(deps, 'libgenerated_fixture.rmeta')));
      delete settings.OUT_DIR;
      const before = hits();
      execFileSync(wrapper, [compiler, '--crate-name', 'downstream', '--crate-type', 'rlib',
        '--edition=2021', '--emit=link,dep-info', '--extern', `generated_fixture=${path.join(deps, 'libgenerated_fixture.rlib')}`,
        '--out-dir', deps, consumer], { env: settings });
      if (name === 'b') assert.equal(hits() - before, 1, 'a new configured build directory must reuse its downstream Rust crate');
    }
    assert.ok(metadata[0].equals(metadata[1]), 'generated Rust metadata must not contain the physical build-directory distinction');
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
