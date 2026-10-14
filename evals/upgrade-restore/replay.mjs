import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {readFileSync, realpathSync, writeFileSync} from 'node:fs';
import {resolve, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {requireBackings, verdict, failureReason} from './model.mjs';

const exec = promisify(execFile);
const [file, expectedBuild, root, deadlineRaw = '90', mode = 'verify'] = process.argv.slice(2);
const deadlineSeconds = Number(deadlineRaw);
if (!file || !expectedBuild || !root || !Number.isFinite(deadlineSeconds) || deadlineSeconds <= 0 || deadlineSeconds > 120) {
  throw new Error('Usage: node replay.mjs DESK_JSON BUILD_SHA FIXTURE_ROOT [DEADLINE_SECONDS<=120]');
}
const document = JSON.parse(readFileSync(file, 'utf8'));
const expected = requireBackings(document);
const benchLib = fileURLToPath(new URL('../_lib/lib.sh', import.meta.url));
const deadline = performance.now() + deadlineSeconds * 1000;
async function dc(...args) {
  const remaining = Math.ceil(deadline - performance.now());
  if (remaining <= 0) throw new Error('bounded replay deadline elapsed');
  const pending = exec('bash', ['-c', 'source "$1"; shift; dc "$@"', 'upgrade-restore', benchLib, ...args],
    {timeout: remaining, killSignal: 'SIGKILL', detached: process.platform !== 'win32', maxBuffer: 32 * 1024 * 1024});
  try {
    const result = await pending;
    return JSON.parse(result.stdout);
  } catch (error) {
    if (error.killed && pending.child.pid) {
      if (process.platform === 'win32') {
        await exec('taskkill', ['/PID', String(pending.child.pid), '/T', '/F'], {timeout: 5000}).catch(() => {});
      } else {
        try { process.kill(-pending.child.pid, 'SIGKILL'); } catch {}
      }
    }
    throw error;
  }
}
const identify = await dc('identify', '--json');
if (!String(identify.connection?.instance ?? '').startsWith('upgrade-restore-')) {
  throw new Error('Replay requires a named upgrade-restore-* DEV instance');
}
const health = await exec('bash', ['-c', 'source "$1"; dev_get /api/health', 'upgrade-restore', benchLib],
  {timeout: Math.max(1, Math.ceil(deadline - performance.now())), maxBuffer: 1024 * 1024});
if (JSON.parse(health.stdout).build !== expectedBuild) throw new Error('Running build SHA differs from required replay SHA');
const providers = await dc('agent', 'providers', '--json');
const provider = providers.find(provider => provider.id === 'codex');
const resolvedBinary = provider?.cli_status?.resolved_path;
const mockBinary = join(resolve(root), 'bin', process.platform === 'win32' ? 'codex.cmd' : 'codex');
if (provider?.agent_runtime?.command?.binary !== 'codex' || !resolvedBinary
  || realpathSync(resolvedBinary) !== realpathSync(mockBinary)) {
  throw new Error('Fake app-server backing is not the DEV daemon’s resolved runtime binary');
}
if (mode === 'stage') {
  const imported = await dc('sessions', 'backups', 'import', '--file', file, '--json');
  if (imported.imported_tabs !== expected.length || imported.restart_required !== true) {
    throw new Error('Import lost tabs or omitted the required restart acknowledgement');
  }
  console.log(JSON.stringify(imported));
  process.exit(0);
}
if (mode !== 'verify') throw new Error('Unknown replay mode');
const results = await Promise.all(expected.map(async tab => {
  try {
    if (tab.agent_type) {
      const chat = await dc('tab', 'chat', '--tab', tab.tab_id, '--json');
      const messages = chat.messages ?? [];
      const history = document.histories.find(history => history.tab_id === tab.tab_id);
      for (const row of history.messages) {
        const observed = messages.find(message => message.id === row.id);
        if (!observed || observed.type !== row.type || Buffer.byteLength(observed.content ?? '') !== Buffer.byteLength(row.content ?? '')) {
          return {tab_id: tab.tab_id, ready: false, reason: `restored history missing or truncated at item ${row.id}`};
        }
      }
    }
    const actual = await dc('tab', 'inspect', '--tab', tab.tab_id, '--json');
    const record = Array.isArray(actual) ? actual[0] : actual;
    const result = verdict(tab, record);
    const live = document.live_tab_metadata.find(row => row.tab_id === tab.tab_id);
    if (live && (live.hidden !== record?.hidden || live.show_in_tabbar !== record?.show_in_tabbar)) {
      return {tab_id: tab.tab_id, ready: false, reason: 'hidden/tabbar visibility changed after restore'};
    }
    return result;
  } catch (error) {
    return {tab_id: tab.tab_id, ready: false, reason: failureReason(error)};
  }
}));
const trace = readFileSync(join(root, 'provider-trace.jsonl'), 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse);
for (const tab of expected.filter(tab => tab.agent_type)) {
  const session = tab.claude_session_id ?? tab.agent_session_id;
  if (!trace.some(row => row.thread_id === session && row.method === 'thread/turns/list')) {
    results.push({tab_id: tab.tab_id, ready: false, reason: 'history backing was not consumed by restored runtime'});
  }
}
const record = {build: expectedBuild, deadline_seconds: deadlineSeconds, tabs: results, passed: results.every(row => row.ready)};
writeFileSync(join(root, 'replay-result.json'), JSON.stringify(record, null, 2));
for (const row of results) console.log(`${row.ready ? 'PASS' : 'FAIL'} tab=${row.tab_id}: ${row.reason}`);
if (!record.passed) process.exitCode = 1;
