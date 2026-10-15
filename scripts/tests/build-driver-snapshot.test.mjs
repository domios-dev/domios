import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import test from 'node:test';

test('a queued build parses its driver before waiting even when the source file changes', { timeout: 10000 }, async (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'build-driver-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const driver = path.join(directory, 'driver.sh');
  const source = fs.readFileSync('build.sh', 'utf8')
    .replace(/^setup_env\(\) \{[\s\S]*?^\}/m, 'setup_env() { printf "driver-ready\\n"; IFS= read -r release; }')
    .replace('    # Already loaded above. Use: source build.sh env', '    printf "dispatch-complete\\n"');
  fs.writeFileSync(driver, source);
  const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
  const child = spawn(bash, [driver, 'env'], { stdio: ['pipe', 'pipe', 'pipe'] });
  t.after(() => child.kill());
  let output = '', error = '';
  child.stderr.on('data', chunk => { error += chunk; });
  const ready = new Promise(resolve => child.stdout.on('data', chunk => {
    output += chunk;
    if (output.includes('driver-ready')) resolve();
  }));
  const exited = once(child, 'exit');
  await Promise.race([ready, exited.then(() => { throw new Error(`Driver exited before wait: ${error}`); })]);
  fs.writeFileSync(driver, '"broken replacement\n');
  child.stdin.end('\n');
  assert.equal((await exited)[0], 0, error);
  assert.ok(output.includes('dispatch-complete'), output);
});
