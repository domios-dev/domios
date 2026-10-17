// Caller: build.sh cache-wrapper-check
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { prepareCacheWrapper } from '../cache-rustc-wrapper.mjs';

test('compiler cache relocates Cargo outputs while preserving tracked environment and failures', {
  skip: process.env.CODETERM_CACHE_WRAPPER_TEST !== '1',
  timeout: 120000,
}, async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ct-rust-cache-'));
  const listener = net.createServer();
  await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve));
  const port = listener.address().port;
  await new Promise(resolve => listener.close(resolve));
  const env = { ...process.env, SCCACHE_SERVER_PORT: String(port), SCCACHE_DIR: path.join(root, 'cache'), SCCACHE_CACHE_SIZE: '64M' };
  for (const name of ['SCCACHE_SERVER_UDS', 'SCCACHE_BUCKET', 'SCCACHE_REDIS', 'SCCACHE_MEMCACHED', 'SCCACHE_GCS_BUCKET', 'SCCACHE_AZURE_CONNECTION_STRING', 'SCCACHE_WEBDAV_ENDPOINT']) delete env[name];
  env.SCCACHE_CONF = path.join(root, 'sccache.toml');
  fs.writeFileSync(env.SCCACHE_CONF, '');
  const wrapper = prepareCacheWrapper({ env });
  const compiler = fs.readFileSync(wrapper.replace(/(?:\.exe)?$/, '.compiler'), 'utf8');
  const run = (program, args, extra = {}) => {
    const childEnv = { ...env, ...extra };
    for (const [name, value] of Object.entries(childEnv)) if (value === undefined) delete childEnv[name];
    return execFileSync(program, args, { cwd: root, env: childEnv, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  };
  const stats = () => JSON.parse(run('sccache', ['--show-stats', '--stats-format', 'json'])).stats;
  const hits = value => value.cache_hits.counts.Rust || 0;
  function compile(source, target, extra = {}) {
    const output = path.join(root, target);
    fs.mkdirSync(output, { recursive: true });
    run(wrapper, [compiler, '--crate-name', 'fixture', '--crate-type', 'rlib', '--emit=link,dep-info', '--out-dir', output, source], {
      CARGO_TARGET_DIR: path.join(root, target), CARGO_INCREMENTAL: '0', ...extra,
    });
    return output;
  }
  try {
    run('sccache', ['--start-server']);
    fs.writeFileSync(path.join(root, 'plain.rs'), 'pub fn value() -> usize { 42 }\n');
    for (const target of ['unwrapped-a', 'unwrapped-b']) {
      const output = path.join(root, target);
      fs.mkdirSync(output);
      run('sccache', [compiler, '--crate-name', 'fixture', '--crate-type', 'rlib', '--emit=link,dep-info', '--out-dir', output, 'plain.rs'], {
        CARGO_TARGET_DIR: output, CARGO_INCREMENTAL: '0',
      });
    }
    assert.equal(hits(stats()), 0, 'ordinary sccache misses when only the Cargo target directory changes');
    compile('plain.rs', 'first');
    const seeded = stats();
    compile('plain.rs', 'second');
    const relocated = stats();
    assert.equal(hits(relocated) - hits(seeded), 1, 'relocated outputs must hit the compiler cache');
    assert(fs.readFileSync(path.join(root, 'first/libfixture.rlib')).equals(fs.readFileSync(path.join(root, 'second/libfixture.rlib'))));

    fs.writeFileSync(path.join(root, 'tracked.rs'), 'pub const TARGET: &str = env!("CARGO_TARGET_DIR");\npub const INCREMENTAL: Option<&str> = option_env!("CARGO_INCREMENTAL");\n');
    const a = compile('tracked.rs', 'tracked-a');
    const beforeTrackedChange = stats();
    const b = compile('tracked.rs', 'tracked-b');
    assert.equal(hits(stats()), hits(beforeTrackedChange), 'env! output must not reuse a different tracked value');
    assert(fs.readFileSync(path.join(a, 'fixture.d'), 'utf8').includes(`env-dep:CARGO_TARGET_DIR=${a}`));
    assert(fs.readFileSync(path.join(b, 'fixture.d'), 'utf8').includes(`env-dep:CARGO_TARGET_DIR=${b}`));
    assert(!fs.readFileSync(path.join(a, 'libfixture.rlib')).equals(fs.readFileSync(path.join(b, 'libfixture.rlib'))));

    const unset = compile('tracked.rs', 'tracked-unset', { CARGO_INCREMENTAL: undefined });
    assert(fs.readFileSync(path.join(unset, 'fixture.d'), 'utf8').includes('env-dep:CARGO_INCREMENTAL\n'));
    fs.writeFileSync(path.join(root, 'broken.rs'), 'compile_error!("expected failure");\n');
    assert.throws(() => compile('broken.rs', 'broken'), error => error.status !== 0 && error.stderr.includes('expected failure'));
    assert.equal(run(wrapper, [compiler, '-vV']), run(compiler, ['-vV']));
    assert.equal(prepareCacheWrapper({ env }), wrapper);
    t.diagnostic('Unwrapped relocation missed; wrapped relocation hit; env!/option_env!, probes and compiler failures preserved.');
  } finally {
    try { run('sccache', ['--stop-server']); } finally { fs.rmSync(root, { recursive: true, force: true }); }
  }
});
