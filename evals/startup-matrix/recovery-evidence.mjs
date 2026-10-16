import { readFileSync, watch, statSync, openSync, readSync, closeSync, appendFileSync } from 'node:fs';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { artifacts, options, devEvidence, assertSameListener } from './evidence-runtime.mjs';
import { userIds, recoveryVerdict, tabAuthority } from './evidence-model.mjs';
import { processExit, settleRecovery } from './recovery-settlement.mjs';
import { captureBounded, parseBootPhase } from './phase-collector.mjs';
import { textContent } from '../restart-endurance/oracle.mjs';

const args = options();
if (!args['--fixture'] || Boolean(args['--dry-run']) === Boolean(args['--force-exit']))
  throw Error('--fixture FILE and exactly one of --dry-run / --force-exit required');
if (process.platform !== 'win32') throw Error('Recovery process ancestry adapter currently supports Windows only; no process will be terminated');
const fixture = JSON.parse(readFileSync(args['--fixture'], 'utf8').replace(/^\uFEFF/, ''));
if (![fixture.tab, fixture.session, fixture.nonce, fixture.marker, fixture.process?.created].every(value => typeof value === 'string' && value) ||
    !Number.isInteger(fixture.process?.pid) || fixture.process.pid <= 0) throw Error('Exact owned-session fixture incomplete');
const { out, save } = artifacts(args['--out']);
const control = new AbortController();
const signal = AbortSignal.any([control.signal, AbortSignal.timeout(120000)]);
const result = { schema: 1, scenario: 'exact-session-recovery', stage: 'initial', tab: fixture.tab, marks: [],
  gaps: ['Historical collector hang cause unproven', 'Transcript comparison covers the returned own-session window; no full-history claim'] };
let watcher;
const processHelper = fileURLToPath(new URL('./recovery-process.ps1', import.meta.url));
async function inspectProcess(name, force = false) {
  const path = join(out, name);
  const command = ['-NoLogo', '-NoProfile', '-File', processHelper, '-Session', fixture.session, '-OutputFile', path];
  if (result.listener) command.push('-ExpectedDevPid', String(result.listener.pid));
  if (force) command.push('-ForceExit', '-ExpectedPid', String(fixture.process.pid), '-ExpectedCreation', fixture.process.created);
  const child = spawn('powershell.exe', command, { windowsHide: true, stdio: 'ignore' });
  await processExit(child, signal);
  return JSON.parse(readFileSync(path, 'utf8').replace(/^\uFEFF/, ''));
}
await captureBounded({ signal, persist: outcome => {
  save('recovery.json', { ...result, ...outcome });
  if (outcome.status !== 'completed' || result.verdict?.status === 'fail') process.exitCode = 1;
}, close: () => { watcher?.close(); control.abort(); }, collect: async () => {
  const dev = await devEvidence(args['--expect-build'], signal);
  result.binarySha = dev.health.build_sha; result.listener = dev.listener;
  const own = (await dev.api('/api/mux/state', { signal })).tabs[fixture.tab];
  if (own?.agent_session_id !== fixture.session || own.session_binding_nonce !== fixture.nonce || own.boot_stage !== 'agent_ready' || !own.pty_id)
    throw Error('Current exact tab authority differs from fixture');
  const beforeChat = await dev.api(`/api/tabs/${fixture.tab}/chat`, { signal });
  const messages = Array.isArray(beforeChat) ? beforeChat : beforeChat.messages ?? beforeChat.data?.messages;
  if (messages.filter(message => (message.kind ?? message.type ?? message.role) === 'user' && textContent(message.content).includes(fixture.marker)).length !== 1)
    throw Error('Unique existing own user probe required; runner never sends input');
  const stable = (await dev.api('/api/mux/state', { signal })).tabs[fixture.tab];
  if (JSON.stringify(tabAuthority(own)) !== JSON.stringify(tabAuthority(stable))) throw Error('Authority changed while reading own transcript');
  const beforeProcess = await inspectProcess('process-before.json');
  if (beforeProcess.process.pid !== fixture.process.pid || beforeProcess.process.created !== fixture.process.created)
    throw Error('Current resumed process identity differs from fixture');
  result.before = { tab: tabAuthority(own), process: beforeProcess.process, userIds: userIds(beforeChat) };
  if (args['--dry-run']) {
    result.mode = 'dry-run'; result.stage = 'fixture_validated';
    result.gaps.push('Recovery and no-replay behavior unexecuted');
    return;
  }
  result.mode = 'force-owned-resumed-process-exit';
  const log = join(process.env.CODETERM_DEV_DATA_DIR, 'logs/codeterm.log');
  let position = statSync(log).size, carry = '', total = 0;
  const recovered = new Promise((resolve, reject) => {
    watcher = watch(log, () => {
      try {
        const end = statSync(log).size;
        if (end < position || end - position > 8 * 1024 * 1024) throw Error('Recovery log rotated or read exceeded bound');
        const bytes = Buffer.alloc(end - position), fd = openSync(log, 'r');
        try { readSync(fd, bytes, 0, bytes.length, position); } finally { closeSync(fd); }
        position = end; total += bytes.length;
        if (total > 16 * 1024 * 1024) throw Error('Recovery log capture exceeded byte bound');
        appendFileSync(join(out, 'recovery.log'), bytes);
        carry += bytes.toString();
        let at;
        while ((at = carry.indexOf('\n')) >= 0) {
          const mark = parseBootPhase(carry.slice(0, at)); carry = carry.slice(at + 1);
          if (mark?.tab !== fixture.tab) continue;
          result.marks.push(mark);
          if (mark.phase === 'AgentReady' && mark.pty !== '-' && mark.pty !== own.pty_id) resolve(mark);
        }
      } catch (error) { reject(error); }
    });
  });
  recovered.catch(() => {});
  result.stage = 'observer_armed';
  save('before-exit.json', result);
  await settleRecovery({
    exit: inspectProcess('process-exited.json', true), recovered,
    readFinal: async () => {
      result.stage = 'reading_recovered_authority';
      const after = (await dev.api('/api/mux/state', { signal })).tabs[fixture.tab];
      const chat = await dev.api(`/api/tabs/${fixture.tab}/chat`, { signal });
      const stable = (await dev.api('/api/mux/state', { signal })).tabs[fixture.tab];
      if (JSON.stringify(tabAuthority(after)) !== JSON.stringify(tabAuthority(stable))) throw Error('Recovered authority changed while reading own transcript');
      const process = await inspectProcess('process-after.json');
      const final = await devEvidence(args['--expect-build'], signal);
      assertSameListener(result.listener, final.listener);
      result.finalListener = final.listener;
      result.after = { tab: tabAuthority(after), process: process.process, userIds: userIds(chat) };
      result.verdict = recoveryVerdict(result.before, result.after);
      if (result.verdict.status === 'incomplete') throw Error('Recovery evidence incomplete');
      result.stage = 'finished';
    },
  });
} });
