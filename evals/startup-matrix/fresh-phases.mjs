import { writeFileSync, mkdirSync, statSync, openSync, readSync, closeSync, watch } from 'node:fs';
import { join, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';
import { root, client } from '../core-journeys/lib/env.mjs';
import { captureBounded, parseBootPhase, phasesForTab } from './phase-collector.mjs';
import { devCli } from './phase-cli.mjs';

const [expected, destination, workspaceName = 'Endure', selected = 'shell,claude,codex'] = process.argv.slice(2);
if (!expected || !destination) throw Error('Usage: fresh-phases.mjs BUILD_SHA OUT [WORKSPACE] [shell,claude,codex]');
if (process.env.CODETERM_API_PORT !== '7686' || !process.env.CODETERM_DEV_DATA_DIR) throw Error('Run under evals/_lib dev_run');
const out = resolve(destination);
mkdirSync(out, { recursive: true });
const controller = new AbortController();
const signal = AbortSignal.any([controller.signal, AbortSignal.timeout(120000)]);
const data = { expected, stage: 'initial', rows: [], marks: [], events: [], limits: { deadlineMs: 120000 } };
let finalized = false;
const persist = outcome => {
  if (finalized) return;
  if (outcome.status !== 'collecting') finalized = true;
  writeFileSync(join(out, 'phases.json'), JSON.stringify({ ...data, ...outcome }, null, 2));
};
const checkpoint = stage => { data.stage = stage; persist({ status: 'collecting' }); };
let watcher, logPosition, logPath, raw = '', carry = '', finish;
const ready = new Promise(resolve => { finish = resolve; });
function adopt() {
  for (const row of data.rows) {
    row.phases = phasesForTab(data.marks, row.id);
    const first = row.phases.find(mark => mark.phase === 'first_output');
    const agent = row.phases.find(mark => mark.phase === 'AgentReady');
    row.firstFrameAtUs = first?.atUs;
    row.agentReadyAtUs = agent?.atUs;
    row.observed = row.kind === 'shell' ? Boolean(first) : Boolean(agent);
  }
  if (data.created && data.rows.every(row => row.observed || row.error)) finish();
}
function readLog() {
  const size = statSync(logPath).size;
  if (size < logPosition) throw Error('DEV log rotated');
  const count = size - logPosition;
  if (!count) return;
  if (count > 20 * 1024 * 1024 || raw.length + count > 40 * 1024 * 1024) throw Error('Log capture exceeded bound');
  const bytes = Buffer.alloc(count), fd = openSync(logPath, 'r');
  try { readSync(fd, bytes, 0, count, logPosition); } finally { closeSync(fd); }
  logPosition = size;
  raw += bytes.toString();
  carry += bytes.toString();
  let newline;
  while ((newline = carry.indexOf('\n')) >= 0) {
    const mark = parseBootPhase(carry.slice(0, newline));
    carry = carry.slice(newline + 1);
    if (mark) data.marks.push(mark);
  }
  adopt();
  writeFileSync(join(out, 'dev-boot-phases.log'), raw);
  persist({ status: 'collecting' });
}

const outcome = await captureBounded({
  signal, persist,
  close: () => { watcher?.close(); controller.abort(Error('Capture closed')); },
  collect: async () => {
    const { api } = client({ url: 'http://127.0.0.1:7686', port: 7686, token: process.env.CODETERM_API_TOKEN });
    checkpoint('health');
    data.health = await api('/api/health', { signal });
    if (data.health.build_sha !== expected) throw Error('Unexpected DEV build');
    checkpoint('workspace');
    const state = await api('/api/mux/state', { signal });
    const workspace = state.workspaces.find(value => value.name === workspaceName);
    if (!workspace) throw Error('Workspace absent');
    logPath = join(process.env.CODETERM_DEV_DATA_DIR, 'logs/codeterm.log');
    logPosition = statSync(logPath).size;
    watcher = watch(logPath, () => {
      try { readLog(); } catch (error) { data.logError = String(error); controller.abort(error); }
    });
    checkpoint('launching');
    await Promise.all(selected.split(',').map(async kind => {
      const row = { kind, requestBeginAt: Date.now() };
      data.rows.push(row);
      const title = `Phase${kind}`;
      let args = ['tab', 'new', '--workspace', workspace.id, '--shell', 'powershell', '--title', title, '--eager'];
      if (kind !== 'shell') {
        row.token = `KEEL_FRESH_${randomUUID()}`;
        const prompt = join(out, `${kind}-prompt.txt`);
        writeFileSync(prompt, `Reply with exactly ${row.token} and no other text. Do not run tools.`);
        args = ['agent', 'spawn', kind, '--workspace', workspace.id, '--title', title, '--non-user-facing', '--task-file', prompt];
      }
      row.command = args;
      try {
        const result = await devCli(root, args, signal);
        row.responseAt = Date.now();
        row.id = result.tab_id ?? result.id ?? result.tab?.id;
        if (!row.id) throw Error('CLI returned no tab id');
      } catch (error) { row.error = String(error); }
      checkpoint('launching');
      adopt();
    }));
    data.created = true;
    readLog();
    adopt();
    checkpoint('readiness');
    await ready;
    checkpoint('final_state');
    const final = await api('/api/mux/state', { signal });
    for (const row of data.rows) {
      const tab = final.tabs[row.id];
      row.final = tab && { stage: tab.boot_stage, session: tab.agent_session_id, nonce: tab.session_binding_nonce, pty: tab.pty_id, backing: tab.backing };
    }
    if (data.rows.some(row => row.error || !row.observed)) throw Error('One or more launches incomplete');
    data.stage = 'finished';
  },
});
console.log(`FRESH_PHASES ${outcome.status} ${out}`);
if (outcome.status !== 'completed') process.exitCode = 1;
