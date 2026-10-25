import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { publishDevExecutable } from '../server-artifact.mjs';

function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev executable publish-'));
  const source = path.join(directory, 'new.exe');
  const destination = path.join(directory, 'server.exe');
  fs.writeFileSync(source, 'new verified image');
  fs.writeFileSync(destination, 'old pinned image');
  return { directory, source, destination };
}

for (const platform of ['win32', 'linux', 'darwin']) {
  test(`DEV executable publication preserves its stable path and skips unchanged images: ${platform}`, () => {
    const { directory, source, destination } = fixture();
    try {
      publishDevExecutable(source, destination, { platform });
      assert.equal(fs.readFileSync(destination, 'utf8'), 'new verified image');
      const files = fs.readdirSync(directory);
      assert.equal(files.some(file => file.endsWith('.pending')), false);
      const retired = files.filter(file => file.endsWith('.retired'));
      assert.equal(retired.length, platform === 'win32' ? 1 : 0);
      if (retired.length) assert.equal(fs.readFileSync(path.join(directory, retired[0]), 'utf8'), 'old pinned image');
      publishDevExecutable(source, destination, { platform, rename() { assert.fail('unchanged image moved'); } });
      assert.deepEqual(fs.readdirSync(directory), files.filter(file => !file.endsWith('.retired')));
    } finally { fs.rmSync(directory, { recursive: true }); }
  });
}

test('failed Windows publication restores the previous image and removes the candidate', () => {
  const { directory, source, destination } = fixture();
  try {
    assert.throws(() => publishDevExecutable(source, destination, {
      platform: 'win32',
      rename(from, to) {
        if (from.endsWith('.pending')) throw Error('injected publication refusal');
        fs.renameSync(from, to);
      },
    }), /publication refusal/);
    assert.equal(fs.readFileSync(destination, 'utf8'), 'old pinned image');
    assert.deepEqual(fs.readdirSync(directory).sort(), ['new.exe', 'server.exe']);
  } finally { fs.rmSync(directory, { recursive: true }); }
});

test('Windows publication prunes only older retired siblings and retains the just-retired image', () => {
  const { directory, source, destination } = fixture();
  try {
    const stale = `${destination}.older.retired`;
    const held = `${destination}.held.retired`;
    const unrelated = path.join(directory, 'other.exe.older.retired');
    const pending = `${destination}.older.pending`;
    const subdirectory = `${destination}.directory.retired`;
    for (const file of [stale, held, unrelated, pending]) fs.writeFileSync(file, 'keep or remove');
    fs.mkdirSync(subdirectory);
    publishDevExecutable(source, destination, { platform: 'win32', remove(file) {
      if (file === held) throw Object.assign(Error('held image'), { code: 'EBUSY' });
      fs.rmSync(file);
    } });
    assert.equal(fs.existsSync(stale), false);
    assert.ok([held, unrelated, pending, subdirectory].every(fs.existsSync));
    const retired = fs.readdirSync(directory).filter(file => file.startsWith('server.exe.') && file.endsWith('.retired') && file !== path.basename(held) && file !== path.basename(subdirectory));
    assert.equal(retired.length, 1);
    assert.equal(fs.readFileSync(path.join(directory, retired[0]), 'utf8'), 'old pinned image');
  } finally { fs.rmSync(directory, { recursive: true }); }
});

test('failed publication leaves older retired siblings intact', () => {
  const { directory, source, destination } = fixture();
  try {
    const stale = `${destination}.older.retired`;
    fs.writeFileSync(stale, 'retained after failure');
    assert.throws(() => publishDevExecutable(source, destination, { platform: 'win32', rename(from, to) {
      if (from.endsWith('.pending')) throw Error('publication failed');
      fs.renameSync(from, to);
    } }), /publication failed/);
    assert.equal(fs.readFileSync(stale, 'utf8'), 'retained after failure');
  } finally { fs.rmSync(directory, { recursive: true }); }
});

test('Windows held retired image survives cleanup and is collected after release on unchanged publication', {
  skip: process.platform !== 'win32', timeout: 15_000,
}, async () => {
  const { directory, source, destination } = fixture();
  const retired = `${destination}.held.retired`;
  fs.writeFileSync(retired, 'held retired bytes');
  const script = path.join(directory, 'hold.ps1');
  fs.writeFileSync(script, `$ErrorActionPreference='Stop'
$pin=[IO.File]::Open($args[0], 'Open', 'Read', [IO.FileShare]::Read)
try {
  [Console]::Out.WriteLine('HELD')
  [Console]::Out.Flush()
  [void][Console]::In.ReadLine()
} finally { $pin.Dispose() }
`);
  const child = spawn('powershell.exe', ['-NoProfile', '-File', script, retired], { windowsHide: true });
  const closed = once(child, 'close');
  let output = '', errors = '';
  child.stderr.on('data', chunk => { errors += chunk; });
  const ready = new Promise((resolve, reject) => {
    child.stdout.on('data', chunk => { output += chunk; if (/HELD\r?\n/.test(output)) resolve(); });
    child.once('error', reject);
    child.once('exit', () => reject(Error(`Holder exited before acknowledgement: ${errors}`)));
  });
  try {
    await ready;
    publishDevExecutable(source, destination);
    assert.equal(fs.readFileSync(retired, 'utf8'), 'held retired bytes');
    child.stdin.end('release\n');
    assert.equal((await closed)[0], 0, errors);
    publishDevExecutable(source, destination);
    assert.equal(fs.existsSync(retired), false);
    assert.equal(fs.readdirSync(directory).some(file => file.endsWith('.retired')), false);
  } finally {
    child.stdin.end();
    if (child.exitCode === null) child.kill();
    await closed;
    fs.rmSync(directory, { recursive: true });
  }
});

test('running Windows retired image survives publication until its process exits', {
  skip: process.platform !== 'win32', timeout: 15_000,
}, async () => {
  const { directory, source, destination } = fixture();
  const retired = `${destination}.running.retired`;
  fs.copyFileSync(process.execPath, retired);
  const child = spawn(retired, ['-e', 'process.stdout.write("READY\\n"); process.stdin.once("data", () => process.exit(0));'], { windowsHide: true });
  const closed = once(child, 'close');
  const ready = once(child.stdout, 'data');
  try {
    await ready;
    publishDevExecutable(source, destination);
    assert.equal(fs.existsSync(retired), true);
    child.stdin.end('exit');
    assert.equal((await closed)[0], 0);
    publishDevExecutable(source, destination);
    assert.equal(fs.existsSync(retired), false);
  } finally {
    child.stdin.end();
    if (child.exitCode === null) child.kill();
    await closed;
    fs.rmSync(directory, { recursive: true });
  }
});

test('Windows read pin refuses byte overwrite but permits publication without releasing the pin', {
  skip: process.platform !== 'win32', timeout: 15_000,
}, async () => {
  const { directory, source, destination } = fixture();
  const script = path.join(directory, 'pin.ps1');
  fs.writeFileSync(script, `$ErrorActionPreference='Stop'
$pin=[IO.File]::Open($args[0], 'Open', 'Read', ([IO.FileShare]::Read -bor [IO.FileShare]::Delete))
try {
  [Console]::Out.WriteLine('PINNED')
  [Console]::Out.Flush()
  [void][Console]::In.ReadLine()
  $pin.Position=0
  $reader=New-Object IO.StreamReader($pin)
  [Console]::Out.WriteLine($reader.ReadToEnd())
} finally { $pin.Dispose() }
`);
  const child = spawn('powershell.exe', ['-NoProfile', '-File', script, destination], { windowsHide: true });
  const closed = once(child, 'close');
  let output = '', errors = '';
  child.stderr.on('data', chunk => { errors += chunk; });
  const pinned = new Promise((resolve, reject) => {
    child.stdout.on('data', chunk => { output += chunk; if (output.includes('PINNED\n') || output.includes('PINNED\r\n')) resolve(); });
    child.once('error', reject);
    child.once('exit', () => reject(Error(`Pin exited before acknowledgement: ${errors}`)));
  });
  try {
    await pinned;
    assert.throws(() => fs.copyFileSync(source, destination), error => ['EBUSY', 'EACCES', 'EPERM'].includes(error.code));
    publishDevExecutable(source, destination);
    assert.equal(fs.readFileSync(destination, 'utf8'), 'new verified image');
    child.stdin.end('release\n');
    const [code] = await closed;
    assert.equal(code, 0, errors);
    assert.match(output, /old pinned image/);
  } finally {
    child.stdin.end();
    if (child.exitCode === null) child.kill();
    await closed;
    fs.rmSync(directory, { recursive: true });
  }
});
