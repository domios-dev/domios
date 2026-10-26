import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm, symlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { bundlePackages, inventory, unpack, verifyReceipt } from '../release-arm64-handoff.mjs';

const expected = { target: 'aarch64-unknown-linux-gnu', tag: 'v1.2.3', source_sha: 'a'.repeat(40), run_id: '17', run_attempt: '2' };
const files = [{ path: 'payload/server', sha256: 'b'.repeat(64) }];
const receipt = () => ({ schema: 1, ...expected, files: structuredClone(files) });

test('handoff binds source, target, tag and producing run attempt', () => {
  assert.deepEqual(verifyReceipt(receipt(), expected, files), receipt());
  for (const key of Object.keys(expected)) {
    assert.throws(() => verifyReceipt({ ...receipt(), [key]: 'foreign' }, expected, files), /Foreign handoff/);
  }
  for (const target of ['x86_64-unknown-linux-gnu', 'aarch64-apple-darwin']) {
    assert.throws(() => verifyReceipt(receipt(), { ...expected, target }, files), /Invalid handoff identity/);
  }
});

test('complete digest inventory refuses changed, omitted or extra bytes', () => {
  for (const actual of [[], [{ ...files[0], sha256: 'c'.repeat(64) }], [...files, { path: 'extra', sha256: 'c'.repeat(64) }]]) {
    assert.throws(() => verifyReceipt(receipt(), expected, actual), /inventory differs/);
  }
});

test('handoff refuses signatures, unsafe paths and malformed digests', () => {
  for (const row of [
    { path: 'payload/setup.exe.sig', sha256: 'b'.repeat(64) },
    { path: '../escape', sha256: 'b'.repeat(64) },
    { path: 'C:/escape', sha256: 'b'.repeat(64) },
    { path: 'payload', sha256: 'short' },
  ]) assert.throws(() => verifyReceipt({ ...receipt(), files: [row] }, expected, [row]), /unsigned|path or digest/);
});

test('inventory hashes nested regular files and rejects links', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'handoff-test-'));
  try {
    await mkdir(path.join(directory, 'nested'));
    await writeFile(path.join(directory, 'nested', 'payload'), 'abc');
    assert.deepEqual(await inventory(directory), [{ path: 'nested/payload', sha256: 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' }]);
    await symlink(path.join(directory, 'nested'), path.join(directory, 'link'), process.platform === 'win32' ? 'junction' : 'dir');
    await assert.rejects(inventory(directory), /contains a link/);
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('handoff carries finished packages and leaves bundler staging trees behind', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'handoff-bundle-'));
  try {
    const root = directory.replaceAll('\\', '/');
    for (const kind of ['appimage/Domios.AppDir', 'deb/Domios_1.2.3_arm64/data', 'nsis']) await mkdir(path.join(directory, kind), { recursive: true });
    for (const file of ['appimage/Domios_1.2.3_aarch64.AppImage', 'deb/Domios_1.2.3_arm64.deb', 'deb/Domios_1.2.3_arm64/data/payload', 'nsis/Domios_1.2.3_arm64-setup.exe']) {
      await writeFile(path.join(directory, file), 'abc');
    }
    await symlink(path.join(directory, 'nsis'), path.join(directory, 'appimage', 'Domios.AppDir', '.DirIcon'), process.platform === 'win32' ? 'junction' : 'dir');
    assert.deepEqual(await bundlePackages(root), [
      `${root}/appimage/Domios_1.2.3_aarch64.AppImage`, `${root}/deb/Domios_1.2.3_arm64.deb`, `${root}/nsis/Domios_1.2.3_arm64-setup.exe`,
    ]);
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('archive digest mismatch refuses before extraction or finalization', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'handoff-archive-'));
  try {
    const archive = path.join(directory, 'invalid.tar');
    await writeFile(archive, 'not a tar archive');
    await assert.rejects(unpack(expected.target, expected.tag, expected.source_sha, archive, 'b'.repeat(64)), /archive digest differs/);
  } finally { await rm(directory, { recursive: true, force: true }); }
});
