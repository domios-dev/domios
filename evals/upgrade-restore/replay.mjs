import {readFileSync, realpathSync, writeFileSync} from 'node:fs';
import {resolve, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {requireBackings, verdict, failureReason, requireRestart} from './model.mjs';

import {run as exec} from './process.mjs';
const [file, expectedBuild, root, deadlineRaw = '90', mode = 'verify'] = process.argv.slice(2);
const deadlineSeconds = Number(deadlineRaw);
if (!file || !expectedBuild || !root || !Number.isFinite(deadlineSeconds) || deadlineSeconds <= 0 || deadlineSeconds > 120) {
  throw new Error('Usage: node replay.mjs DESK_JSON BUILD_SHA FIXTURE_ROOT [DEADLINE_SECONDS<=120]');
}
const fixtureBytes = readFileSync(file);
const fixtureDigest = createHash('sha256').update(fixtureBytes).digest('hex');
const document = JSON.parse(fixtureBytes);
const expected = requireBackings(document);
const benchLib = fileURLToPath(new URL('../_lib/lib.sh', import.meta.url));
const deadline = performance.now() + deadlineSeconds * 1000;
async function dc(...args) {
  const remaining = Math.ceil(deadline - performance.now());
  if (remaining <= 0) throw new Error('bounded replay deadline elapsed');
  const result = await exec('bash', ['-c', 'source "$1"; shift; dc "$@"', 'upgrade-restore', benchLib, ...args],
    {timeout: remaining, maxBuffer: 32 * 1024 * 1024});
  return JSON.parse(result.stdout);
}
const identify = await dc('identify', '--json');
if (!String(identify.connection?.instance ?? '').startsWith('upgrade-restore-')) {
  throw new Error('Replay requires a named upgrade-restore-* DEV instance');
}
const health = await exec('bash', ['-c', 'source "$1"; dev_get /api/health', 'upgrade-restore', benchLib],
  {timeout: Math.max(1, Math.ceil(deadline - performance.now())), maxBuffer: 1024 * 1024});
const currentHealth = JSON.parse(health.stdout);
if (currentHealth.build_sha !== expectedBuild) throw new Error('Running full build SHA differs from required replay SHA');
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
  if (!imported.boot_id) throw new Error('Import omitted the daemon boot identity');
  writeFileSync(join(root, 'stage-result.json'), JSON.stringify({...imported,
    fixture_sha256: fixtureDigest, build_sha: currentHealth.build_sha}, null, 2));
  console.log(JSON.stringify(imported));
  process.exit(0);
}
if (mode !== 'verify') throw new Error('Unknown replay mode');
requireRestart(JSON.parse(readFileSync(join(root, 'stage-result.json'), 'utf8')),
  currentHealth.boot_id, fixtureDigest);
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
const record = {build: expectedBuild, boot_id: currentHealth.boot_id, fixture_sha256: fixtureDigest,
  deadline_seconds: deadlineSeconds, tabs: results, passed: results.every(row => row.ready)};
writeFileSync(join(root, 'replay-result.json'), JSON.stringify(record, null, 2));
for (const row of results) console.log(`${row.ready ? 'PASS' : 'FAIL'} tab=${row.tab_id}: ${row.reason}`);
if (!record.passed) process.exitCode = 1;
