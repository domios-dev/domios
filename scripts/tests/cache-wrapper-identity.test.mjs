import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';

test('identical compiler wrappers share identity across checkout paths; changed source stays isolated', async () => {
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
