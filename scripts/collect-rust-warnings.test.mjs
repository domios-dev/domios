import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { Writable } from 'node:stream';
import test from 'node:test';
import { captureCommand } from './ci-rust-tests.mjs';
import { capture, parseWarnings } from './collect-rust-warnings.mjs';

const WARNING = 'warning: unused variable: `x`\n --> src/lib.rs:3:9\n';
const RETURN_BOUND_MS = 15_000;

// A step that leaves a detached descendant (like a lazily started sccache server) holding its stdout, then exits 3.
const STEP = String.raw`
const { spawn } = require('node:child_process');
const descendant = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 120000)'], { detached: true, stdio: ['ignore', 'inherit', 'inherit'] });
require('node:fs').writeFileSync(process.argv[2], String(descendant.pid));
process.stdout.write(process.argv[3]);
process.exit(3);
`;

function lingeringStep(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrapped-step-'));
  const pidFile = path.join(dir, 'descendant.pid');
  t.after(() => {
    try { process.kill(Number(fs.readFileSync(pidFile, 'utf8'))); } catch { /* already gone */ }
    fs.rmSync(dir, { recursive: true, force: true });
  });
  fs.writeFileSync(path.join(dir, 'step.cjs'), STEP);
  return { dir, args: [path.join(dir, 'step.cjs'), pidFile, WARNING], log: path.join(dir, 'step.log') };
}

const sink = () => { const chunks = []; return { stream: new Writable({ write(chunk, _, done) { chunks.push(chunk); done(); } }), text: () => Buffer.concat(chunks).toString() }; };

async function returnsInTime(promise) {
  let timer;
  const deadline = new Promise((_, reject) => { timer = setTimeout(() => reject(Error('wrapper outlived its child')), RETURN_BOUND_MS); });
  try { return await Promise.race([promise, deadline]); } finally { clearTimeout(timer); }
}

test('the warnings wrapper returns its child exit code while a descendant still holds stdout', async t => {
  const step = lingeringStep(t);
  const stdout = sink(), stderr = sink();
  const code = await returnsInTime(capture(process.execPath, step.args, step.log, { drainMs: 200, stdout: stdout.stream, stderr: stderr.stream }));
  assert.equal(code, 3);
  assert.equal(stdout.text(), WARNING);
  assert.deepEqual(parseWarnings(fs.readFileSync(step.log, 'utf8'), { target: 't' }).map(w => [w.file, w.line, w.code]), [['src/lib.rs', 3, 'unknown']]);
  assert.match(stderr.text(), /descendant still held/);
});

test('the nextest wrapper returns its child status while a descendant still holds stdout', async t => {
  const step = lingeringStep(t);
  const result = await returnsInTime(captureCommand(process.execPath, step.args, step.log, process.env, false, () => {}, 200));
  assert.equal(result.status, 3);
  assert.equal(result.signal, null);
  assert.equal(fs.readFileSync(step.log, 'utf8'), WARNING);
});
