import assert from 'node:assert/strict';
import test from 'node:test';
import { assertMachine, machine, checkContents } from '../release-bundle-smoke.mjs';
function executable(format, arch) {
  const bytes = Buffer.alloc(128);
  if (format === 'PE') { bytes.write('MZ'); bytes.writeUInt32LE(64, 60); bytes.write('PE\0\0', 64, 'binary'); bytes.writeUInt16LE(arch === 'arm64' ? 0xaa64 : 0x8664, 68); }
  if (format === 'ELF') { Buffer.from([0x7f,0x45,0x4c,0x46,2,1]).copy(bytes); bytes.writeUInt16LE(arch === 'arm64' ? 183 : 62,18); }
  if (format === 'Mach-O') { bytes.writeUInt32LE(0xfeedfacf); bytes.writeUInt32LE(arch === 'arm64' ? 0x0100000c : 0x01000007,4); }
  return bytes;
}
for (const format of ['PE','ELF','Mach-O']) {
  test(`${format} rejects mixed app/CLI architectures`, () => {
    for (const arch of ['x64','arm64']) {
      const bytes = executable(format,arch);
      assert.deepEqual(machine(bytes),{format,arch});
      assertMachine(bytes,arch,format);
      assert.throws(() => assertMachine(bytes,arch === 'x64' ? 'arm64' : 'x64',format));
    }
  });
}
test('truncated, malformed and unknown executable headers fail closed', () => {
  for (const bytes of [Buffer.alloc(8),Buffer.alloc(128),Buffer.from('MZ'.padEnd(128,'x'))]) assert.throws(()=>machine(bytes));
  const pe=executable('PE','x64'); pe.writeUInt32LE(500,60); assert.throws(()=>machine(pe));
});
test('source SHA and embedded plugins must be present in the app itself', () => {
  assert.throws(()=>checkContents(Buffer.from('app'), 'a'.repeat(40)), /source SHA/);
  assert.throws(()=>checkContents(Buffer.from('a'.repeat(40)), 'a'.repeat(40)), /plugin/);
});
