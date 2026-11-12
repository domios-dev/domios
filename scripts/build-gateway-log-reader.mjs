import fs from 'node:fs';
import path from 'node:path';
import { once } from 'node:events';
import { PassThrough } from 'node:stream';
import { filterFingerprint } from './build-gateway-output.mjs';

const [log, done, stuck, fingerprintArtifact] = process.argv.slice(2);
const output = fingerprintArtifact ? new PassThrough() : process.stdout;
const filtered = fingerprintArtifact ? filterFingerprint(output, process.stdout, fingerprintArtifact) : null;
let changed = false;
let wake;
const watcher = fs.watch(path.dirname(log), () => {
  changed = true;
  wake?.();
});
watcher.on('error', error => { console.error(error); process.exit(74); });
const file = fs.openSync(log, 'r');
let offset = 0;
const buffer = Buffer.alloc(64 * 1024);
try {
  for (;;) {
    changed = false;
    const complete = stuck !== '1' && fs.existsSync(done);
    const end = fs.fstatSync(file).size;
    if (end < offset) throw Error('Gateway log was truncated');
    while (offset < end) {
      const count = fs.readSync(file, buffer, 0, Math.min(buffer.length, end - offset), offset);
      if (!count) throw Error('Gateway log ended before its recorded size');
      offset += count;
      if (!output.write(Buffer.from(buffer.subarray(0, count)))) await once(output, 'drain');
    }
    if (complete) break;
    if (!changed) await new Promise(resolve => { wake = resolve; });
    wake = undefined;
  }
  if (filtered) {
    output.end();
    if ((await filtered).degraded) throw Error('Fingerprint evidence could not be retained or delivered');
  }
} catch (error) {
  console.error(`ERROR: build gateway evidence gap: ${error.message}`);
  process.exitCode = 74;
} finally {
  watcher.close();
  fs.closeSync(file);
}
