import assert from 'node:assert/strict';
import test from 'node:test';
import { assertMachine, machine, checkContents, packagedExecutables } from '../release-bundle-smoke.mjs';

test('package selection separates domios native client from codeterm CLI on every platform', () => {
  for (const [app, cli, server] of [
    ['/usr/bin/domios', '/usr/lib/CodeTerm/cli/codeterm', '/usr/lib/CodeTerm/domios-server/domios-server'],
    ['C:\\package\\domios.exe', 'C:\\package\\cli\\codeterm.exe', 'C:\\package\\domios-server\\domios-server.exe'],
    ['/CodeTerm.app/Contents/MacOS/domios', '/CodeTerm.app/Contents/Resources/cli/codeterm', '/CodeTerm.app/Contents/MacOS/domios-server'],
  ]) {
    assert.deepEqual(packagedExecutables([app, cli, server]), { app, cli, server });
    assert.throws(() => packagedExecutables([cli, server]), /app executable, found 0/);
    assert.throws(() => packagedExecutables([app, cli, app, server]), /app executable, found 2/);
    assert.throws(() => packagedExecutables([app, cli]), /packaged server, found 0/);
  }
});
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
test('the app carries the source SHA and the server it starts carries the embedded plugins', () => {
  const sha = 'a'.repeat(40);
  assert.throws(()=>checkContents(Buffer.from('app'), Buffer.from(sha), sha), /source SHA/);
  assert.throws(()=>checkContents(Buffer.from(sha), Buffer.from('server'), sha), /plugin/);
});
