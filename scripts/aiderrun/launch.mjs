import { spawnSync } from 'node:child_process';
import { writeFileSync, existsSync, openSync, closeSync, unlinkSync } from 'node:fs';
import path from 'node:path';
import { options, cli, call, shellCommand, storage, readJson, save, harness, fingerprint } from './runtime.mjs';

import { decision } from './model.mjs';
import { observed } from './turn.mjs';

const binding = options(process.argv.slice(2));
const binary = cli();
const root = storage(binding);
const turn = observed(binding, root);
const state = readJson(path.join(root, 'state.json'), { claimed: [] });
if (!turn || decision({ ...state, active: existsSync(path.join(root, 'active.lock')) }, turn.key, fingerprint(binding.lane)) === 'waiting') process.exit(0);
const admission = path.join(root, 'launch.lock');
let admissionFd, duplicate = false;
try { admissionFd = openSync(admission, 'wx'); } catch (error) { if (error.code === 'EEXIST') process.exit(0); throw error; }
try {
  const file = path.join(root, 'dispatched.json');
  const dispatched = readJson(file, []);
  duplicate = dispatched.includes(turn.key);
  if (!duplicate) { dispatched.push(turn.key); save(file, dispatched); }
} finally { closeSync(admissionFd); unlinkSync(admission); }
if (duplicate) process.exit(0);
process.env.CODETERM_TAB_ID = binding.orchestrator;
const command = shellCommand([process.execPath, path.join(harness, 'lane-check.mjs'), ...process.argv.slice(2)]);
const started = performance.now();
const pad = call(binary, ['pad', 'path'], binding.lane, 1000);
const result = spawnSync(binary, ['tab', 'new', '--purpose', 'qa', '--team', 'team_dd6d7d99', '--cwd', binding.lane, '--title', 'Lanecheck', '--eager', '--cmd', command, '--json'], { encoding: 'utf8', timeout: 6500, windowsHide: true });
const evidence = { durationMs: performance.now() - started, code: result.status, error: result.error?.message, stdout: result.stdout, stderr: result.stderr };

writeFileSync(path.join(pad, 'lane-launch.json'), JSON.stringify(evidence, null, 2), 'utf8');
console.log(JSON.stringify(evidence));
if (result.error || result.status !== 0 || evidence.durationMs >= 10000) process.exitCode = 1;






