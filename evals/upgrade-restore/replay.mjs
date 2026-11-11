import {bashInterpreter} from '../../scripts/bash-interpreter.mjs';
import {readFileSync, realpathSync, writeFileSync} from 'node:fs';
import {resolve, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {requireBackings, verdict, failureReason, requireRestart, historyMatches, synthesizeHistory} from './model.mjs';
import {cliResult} from './cli-result.mjs';
import {CONNECT_SCRIPT, devApi, isolatedConnection} from './transport.mjs';
import {Events} from '../core-journeys/lib/env.mjs';
import {observe, touchesTab} from '../core-journeys/lib/wait.mjs';

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
  return cliResult(exec, bashInterpreter(), ['-c', 'source "$1"; shift; dc "$@"', 'upgrade-restore', benchLib, ...args],
    {timeout: remaining, maxBuffer: 32 * 1024 * 1024});
}
const connection = await exec(bashInterpreter(), ['-c', CONNECT_SCRIPT, 'upgrade-restore', benchLib],
  {timeout: Math.max(1, Math.ceil(deadline - performance.now())), maxBuffer: 1024 * 1024});
const dev = isolatedConnection(JSON.parse(connection.stdout.trim().split('\n').at(-1)));
const daemon = devApi(dev, deadline);
const currentHealth = await daemon.health();
if (currentHealth.instance?.name !== dev.instance) throw new Error('Daemon instance differs from the resolved DEV instance');
if (currentHealth.build_sha !== expectedBuild) throw new Error('Running full build SHA differs from required replay SHA');
const providers = await daemon.providers();
const provider = providers.find(provider => provider.id === 'codex');
const resolvedBinary = provider?.cli_status?.resolved_path;
const mockBinary = join(resolve(root), 'bin', process.platform === 'win32' ? 'codex.cmd' : 'codex');
if (provider?.agent_runtime?.command?.binary !== 'codex' || !resolvedBinary
  || realpathSync(resolvedBinary) !== realpathSync(mockBinary)) {
  throw new Error('Fake app-server backing is not the DEV daemon’s resolved runtime binary');
}
if (mode === 'stage') {
  await dc('config', 'set', 'general_agent.enabled', 'false', '--json');
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
  const events = new Events(dev, AbortSignal.timeout(Math.max(1, Math.ceil(deadline - performance.now()))));
  try {
    await events.open();
    await daemon.focus(tab.tab_id);
    await observe({events, filter: touchesTab(tab.tab_id), label: `tab=${tab.tab_id}: restored runtime readiness`,
      read: () => daemon.inspect(tab.tab_id),
      judge: actual => {
        const result = verdict(tab, actual);
        if (result.ready) return {done: actual};
        if (actual?.agent_runtime_state?.state === 'degraded' || actual?.boot_stage === 'spawn_failed') {
          return {fail: result.reason};
        }
        return {pending: `tab=${tab.tab_id}: ${result.reason}`};
      },
    });
    if (tab.agent_type) {
      const chat = await daemon.chat(tab.tab_id);
      const messages = chat.messages ?? [];
      const history = document.histories.find(history => history.tab_id === tab.tab_id);
      for (const row of synthesizeHistory(history.shape)) {
        const observed = messages.find(message => message.id === row.id);
        if (!historyMatches(row, observed)) {
          return {tab_id: tab.tab_id, ready: false, reason: `restored history missing or truncated at item ${row.id}`};
        }
      }
    }
    const record = await daemon.inspect(tab.tab_id);
    const result = verdict(tab, record);
    const live = document.live_tab_metadata.find(row => row.tab_id === tab.tab_id);
    if (live && (live.hidden !== record?.hidden || live.show_in_tabbar !== record?.show_in_tabbar)) {
      return {tab_id: tab.tab_id, ready: false, reason: 'hidden/tabbar visibility changed after restore'};
    }
    return result;
  } catch (error) {
    return {tab_id: tab.tab_id, ready: false, reason: failureReason(error)};
  } finally {
    events.close();
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
