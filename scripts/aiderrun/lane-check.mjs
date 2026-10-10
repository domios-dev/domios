import { openSync, closeSync, unlinkSync, existsSync } from 'node:fs';
import path from 'node:path';
import { commands, verdict, missingTarget, decision, followUp } from './model.mjs';
import { spawnSync } from 'node:child_process';
import { options, cli, fingerprint, storage, readJson, save, harness } from './runtime.mjs';
import { observed } from './turn.mjs';
import { run } from './process.mjs';
import { deliver, diagnosticResults } from './verdict.mjs';

const binding = options(process.argv.slice(2));
const binary = cli(), root = storage(binding), lock = path.join(root, 'active.lock');
let fd, release = true, deliveryAttempted = false, checkedKey = null;
try { fd = openSync(lock, 'wx'); } catch (error) { if (error.code === 'EEXIST') process.exit(0); throw error; }
try {
  const turn = observed(binding, root);
  if (!turn) throw new Error('no verified completed turn; checks were not started');
  checkedKey = turn.key;
  const hash = fingerprint(binding.lane), stateFile = path.join(root, 'state.json');
  const state = readJson(stateFile, { claimed: [] });
  if (decision({ ...state, active: false }, turn.key, hash) === 'ready') {
    state.claimed.push(turn.key);
    save(stateFile, state);
    const results = [];
    const end = Date.now() + 180000;
    if (!existsSync(path.join(binding.lane, 'node_modules'))) results.push({ code: null, error: 'missing lane node_modules; run pnpm install --frozen-lockfile --offline inside the lane' });
    else if (!turn.complete) results.push({ code: null, error: `Aider outcome: ${turn.outcome}` });
    else for (const command of commands(binding.surface)) {
      const target = missingTarget(command, arg => existsSync(path.join(binding.lane, arg))); 
      if (target && !existsSync(path.join(binding.lane, target))) { results.push({ code: null, error: `missing target: ${target}` }); break; }
      if (Date.now() >= end) { results.push({ code: null, timeout: true }); break; }
      const result = await run(command, binding.lane, end - Date.now());
      results.push(result);
      if (result.contained === false) release = false;
      if (verdict([result]) !== 'PASSED') break;
    }
    if (fingerprint(binding.lane) !== hash || observed(binding, root)?.key !== turn.key) results.push({ code: null, error: 'lane or turn changed during checks' });
    const content = `${verdict(results)}\nrun=${turn.key}\nworker=${binding.worker}\nlane=${binding.lane}\nfingerprint=${hash}\n${JSON.stringify(diagnosticResults(results), null, 2)}\n`;
    state.lastHash = hash;
    save(stateFile, state);
    deliveryAttempted = true;
    const routes = deliver(binary, binding, root, content);
    process.stdout.write(content);
    process.exitCode = verdict(results) === 'PASSED' && routes.every(route => !route.unconfirmed) ? 0 : 1;
  }
} catch (error) {
  const content = `FAILED\nworker=${binding.worker}\nlane=${binding.lane}\n${String(error)}\n`;
  try { if (!deliveryAttempted) deliver(binary, binding, root, content); process.stdout.write(content); }
  catch (routingError) { process.stderr.write(`routing unconfirmed: ${routingError}\n`); }
  process.exitCode = 1;
} finally { closeSync(fd); if (release) unlinkSync(lock); }
if (release) {
  let next = null;
  try { next = observed(binding, root)?.key; } catch { next = null; }
  if (followUp(checkedKey, next)) spawnSync(process.execPath, [path.join(harness, 'launch.mjs'), ...process.argv.slice(2)], { stdio: 'inherit', windowsHide: true });
}





