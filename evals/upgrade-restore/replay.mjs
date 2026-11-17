import {bashInterpreter} from '../../scripts/bash-interpreter.mjs';
import {existsSync, readFileSync, realpathSync, writeFileSync} from 'node:fs';
import {resolve, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {requireBackings, verdict, failureReason, requireRestart, historyMatches, synthesizeHistory, capturedUnavailable} from './model.mjs';
import {unavailableVerdict} from './unavailable.mjs';
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
for (const providerId of new Set(expected.filter(tab => tab.agent_type).map(tab => tab.agent_type))) {
  const provider = providers.find(row => row.id === providerId);
  const resolvedBinary = provider?.cli_status?.resolved_path;
  const executable = process.platform === 'win32' ? `${providerId}.cmd` : providerId;
  const mockBinary = join(resolve(root), 'bin', executable);
  if (provider?.agent_runtime?.command?.binary !== providerId || !resolvedBinary
    || realpathSync(resolvedBinary) !== realpathSync(mockBinary)) {
    throw new Error(`Fake ${providerId} backing is not the DEV daemon’s resolved runtime binary`);
  }
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
const active = expected.filter(tab => !capturedUnavailable(document, tab));
const passive = expected.filter(tab => capturedUnavailable(document, tab));
const results = await Promise.all(active.map(async tab => {
  const events = new Events(dev, AbortSignal.timeout(Math.max(1, Math.ceil(deadline - performance.now()))));
  try {
    await events.open();
    await daemon.select(tab.tab_id);
    await observe({events, filter: touchesTab(tab.tab_id), label: `tab=${tab.tab_id}: restored runtime readiness`,
      read: () => daemon.inspect(tab.tab_id),
      judge: actual => {
        const result = verdict(tab, actual);
        if (result.ready) return {done: actual};
        if (result.terminal || actual?.agent_runtime_state?.state === 'degraded' || actual?.boot_stage === 'spawn_failed') {
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
const claudeTracePath = join(root, 'claude-trace.jsonl');
const claudeTrace = existsSync(claudeTracePath)
  ? readFileSync(claudeTracePath, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse)
  : [];
const shellTracePath = join(root, 'shell-trace.jsonl');
const shellTrace = existsSync(shellTracePath)
  ? readFileSync(shellTracePath, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse)
  : [];
for (const tab of active.filter(tab => !tab.agent_type && !tab.surface)) {
  const actual = await daemon.inspect(tab.tab_id);
  const launches = shellTrace.filter(row => row.tab_id === tab.tab_id && row.cwd === actual.cwd
    && row.nonce && row.nonce === actual.session_binding_nonce);
  let live = false;
  for (const launch of launches) {
    try { process.kill(launch.pid, 0); live = true; } catch {}
  }
  if (!live) results.push({tab_id: tab.tab_id, ready: false, reason: 'fixture shell has no live own-session PTY process'});
}
for (const tab of active.filter(tab => tab.agent_type)) {
  const session = tab.claude_session_id ?? tab.agent_session_id;
  if (tab.agent_type === 'codex' && !trace.some(row => row.thread_id === session && row.method === 'thread/turns/list')) {
    results.push({tab_id: tab.tab_id, ready: false, reason: 'history backing was not consumed by restored runtime'});
  }
  if (tab.agent_type === 'claude') {
    const actual = await daemon.inspect(tab.tab_id);
    const launches = claudeTrace.filter(row => row.session_id === session && row.cwd === actual.cwd && row.pid);
    let live = false;
    for (const launch of launches) {
      try { process.kill(launch.pid, 0); live = true; } catch {}
    }
    if (!live) results.push({tab_id: tab.tab_id, ready: false,
      reason: 'fake Claude did not resume its own isolated session backing'});
  }
}
const preservedUnavailable = await Promise.all(passive.map(async tab => {
  try {
    const actual = await daemon.inspect(tab.tab_id);
    const result = unavailableVerdict(document, tab, actual, trace);
    const live = document.live_tab_metadata.find(row => row.tab_id === tab.tab_id);
    if (live && (live.hidden !== actual?.hidden || live.show_in_tabbar !== actual?.show_in_tabbar)) {
      return {...result, preserved: false, reason: 'hidden/tabbar visibility changed after restore'};
    }
    return result;
  } catch (error) {
    return {tab_id: tab.tab_id, bucket: 'preserved-unavailable', preserved: false, reason: failureReason(error)};
  }
}));
const record = {build: expectedBuild, boot_id: currentHealth.boot_id, fixture_sha256: fixtureDigest,
  deadline_seconds: deadlineSeconds, tabs: results, preserved_unavailable: preservedUnavailable,
  passed: results.every(row => row.ready) && preservedUnavailable.every(row => row.preserved)};
writeFileSync(join(root, 'replay-result.json'), JSON.stringify(record, null, 2));
for (const row of results) console.log(`${row.ready ? 'PASS' : 'FAIL'} tab=${row.tab_id}: ${row.reason}`);
for (const row of preservedUnavailable) console.log(`${row.preserved ? 'PRESERVED' : 'FAIL'} tab=${row.tab_id} [preserved-unavailable]: ${row.reason}`);
if (!record.passed) process.exitCode = 1;
