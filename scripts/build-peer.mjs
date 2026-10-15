#!/usr/bin/env node
// Caller: build.sh check-queue|nextest --peer, check-linux, peer-setup, peer-status, peer-route; scripts build-peer.test.mjs; skill build
import fs from 'node:fs';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import {
  LOCK_NAME, VERBS, findPeer, jobEvent, loadPeers, newJobId, outcomeOf, parseDispatchArgs, parseDuration,
  parseJobLog, parseSeriesSpec, parseStatusOutput, pathPrefix, seriesSummary, pickPeer, remoteCommand, STATUS_MARKER, summarizeGateway,
} from './build-peer-lib.mjs';
import { gatewayDirOf, otherGatewayJobs, runSeries } from './build-peer-series.mjs';

const ROOT = path.resolve(import.meta.dirname, '..');
const PROBE_TIMEOUT_MS = 20000;
const SEND_TIMEOUT_MS = 60000;

class Refusal extends Error {
  constructor(message, code = 2) { super(message); this.code = code; }
}

function cliCommand(args) {
  const bin = process.env.CODETERM_PEER_CLI || 'codeterm';
  return bin.endsWith('.mjs') ? [process.execPath, [bin, ...args]] : [bin, args];
}

function cli(args, { input, timeoutMs = PROBE_TIMEOUT_MS } = {}) {
  const [cmd, argv] = cliCommand(args);
  const result = spawnSync(cmd, argv, { encoding: 'utf8', input, timeout: timeoutMs, windowsHide: true });
  return { status: result.error ? 1 : result.status, stdout: result.stdout ?? '', stderr: result.stderr || (result.error?.message ?? '') };
}

function cliAsync(args, timeoutMs) {
  const [cmd, argv] = cliCommand(args);
  return new Promise((resolve) => {
    const child = spawn(cmd, argv, { windowsHide: true });
    let stdout = '';
    let stderr = '';
    const deadline = setTimeout(() => child.kill(), timeoutMs);
    child.stdout.on('data', (d) => { stdout += d; });
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('error', (error) => { clearTimeout(deadline); resolve({ status: 1, stdout, stderr: error.message }); });
    child.on('close', (status) => { clearTimeout(deadline); resolve({ status: status ?? 1, stdout, stderr }); });
  });
}

function parseJson(text, what) {
  const start = text.indexOf('{');
  try {
    return JSON.parse(text.slice(start));
  } catch {
    throw new Refusal(`${what} returned no JSON: ${text.trim().slice(0, 200)}`);
  }
}

function git(args, { allowFail = false, status = false, env, input, raw = false } = {}) {
  const result = spawnSync('git', args, { encoding: 'utf8', windowsHide: true, input, env: env ? { ...process.env, ...env } : undefined });
  if (status) return result.status === 0;
  if (result.status !== 0 && !allowFail) throw new Refusal(`git ${args.join(' ')} failed: ${(result.stderr || '').trim()}`);
  return raw ? result.stdout ?? '' : (result.stdout ?? '').trim();
}

function peersConfig() {
  const file = process.env.CODETERM_PEERS_CONFIG || path.join(ROOT, '.codeterm/build-machine/peers.json');
  return loadPeers(JSON.parse(fs.readFileSync(file, 'utf8')), process.env);
}

function takeOption(argv, name) {
  const index = argv.indexOf(name);
  if (index < 0) return null;
  const value = argv[index + 1];
  if (!value) throw new Refusal(`${name} requires a value`);
  argv.splice(index, 2);
  return value;
}

async function probePeer(peer, statusTabTitle) {
  const list = await cliAsync(['tab', 'list', '--mesh', peer.name, '--json'], PROBE_TIMEOUT_MS);
  if (list.status !== 0) return { peer, status: null, error: 'unreachable' };
  let tab;
  try {
    tab = (parseJson(list.stdout, 'tab list').tabs ?? []).find((t) => t.title === statusTabTitle);
  } catch {
    return { peer, status: null, error: 'bad_tab_list' };
  }
  if (!tab) return { peer, status: null, error: 'not_setup' };
  const exec = await cliAsync(['tab', 'exec', '--mesh', peer.name, '--tab', tab.id, '--json',
    '--timeout', String(PROBE_TIMEOUT_MS), `${pathPrefix(peer)}./build.sh peer-status`], PROBE_TIMEOUT_MS + 10000);
  let data;
  try {
    const body = parseJson(exec.stdout, 'tab exec');
    data = body.data ?? body;
  } catch {
    return { peer, status: null, error: 'exec_failed' };
  }
  // Some peer shells lack the prompt marks tab exec needs, so a printed status line counts even when completion reads timeout.
  const status = parseStatusOutput(data.output);
  if (status) return { peer, status, error: null };
  return { peer, status: null, error: data.completion === 'command_finished' ? 'no_status_line' : `exec_${data.completion ?? 'failed'}` };
}

async function route(config, os) {
  const candidates = config.peers.filter((p) => p.routable && (!os || p.os === os));
  const probed = await Promise.all(candidates.map((peer) => probePeer(peer, config.statusTabTitle)));
  const choice = pickPeer(probed, os);
  if (choice.error) throw new Refusal(`peer routing: ${choice.error}; ${choice.detail}`, choice.error === 'busy' ? 75 : 2);
  console.log(`peer-route: chose ${choice.peer.name}; ${choice.detail}`);
  return choice.peer;
}

function resolveSha(ref) {
  const sha = git(['rev-parse', '--verify', `${ref}^{commit}`]);
  const remotes = git(['branch', '-r', '--contains', sha]);
  if (!remotes) throw new Refusal(`${sha} is on no remote-tracking branch; push it before a peer fetches it`);
  return sha;
}

function originIdentity() {
  const mesh = cli(['mesh', 'status', '--json']);
  if (mesh.status !== 0) throw new Refusal(`mesh status failed: ${mesh.stderr.trim()}`);
  const body = parseJson(mesh.stdout, 'mesh status');
  if (!body.instance_id) throw new Refusal('mesh status has no instance_id');
  return { id: body.instance_id, name: body.instance_name ?? body.instance_id };
}

function writeRecord(stateDir, name, record) {
  fs.mkdirSync(stateDir, { recursive: true });
  fs.writeFileSync(path.join(stateDir, `${name}.json`), `${JSON.stringify(record, null, 2)}\n`);
}

function launch({ config, peer, verb, sha, runnerTail, deadlineSecs, replyTab, clone, stateDir, recordNames }) {
  const origin = originIdentity();
  const jobId = newJobId();
  const reply = { peer: origin.id, tab: replyTab };
  const runner = [...runnerTail.head, '--id', jobId, '--peer-name', peer.name, '--verb', verb, '--sha', sha,
    '--reply-peer', reply.peer, '--reply-tab', reply.tab, '--deadline-secs', String(deadlineSecs), ...runnerTail.rest];
  const command = remoteCommand({ peer, repository: config.repository, jobId, verb, sha, clone, runner, reply });
  // A mesh tab cannot carry --timeout (it needs a local owning agent); the runner and git low-speed aborts bound the job.
  const tabArgs = ['tab', 'new', '--mesh', peer.name, '--json', '--no-team', '--eager', '--purpose', 'build', '--title', 'Peerjob',
    '--description', `${verb} ${sha.slice(0, 9)} for ${origin.name}`];
  if (peer.shell) tabArgs.push('--shell', peer.shell);
  tabArgs.push('--cmd', command);
  const created = cli(tabArgs, { timeoutMs: 60000 });
  if (created.status !== 0) throw new Refusal(`peer ${peer.name} refused the job tab: ${(created.stderr || created.stdout).trim().slice(0, 300)}`);
  const tab = parseJson(created.stdout, 'tab new');
  const record = { job_id: jobId, peer: peer.name, verb, sha, remote_tab: tab.tab_id ?? tab.id, reply_tab: replyTab,
    deadline_secs: deadlineSecs, dispatched_at: new Date().toISOString() };
  for (const name of [jobId, ...recordNames]) writeRecord(stateDir, name, record);
  console.log(`peer-job: id=${jobId} peer=${peer.name} verb=${verb} sha=${sha} remote-tab=${record.remote_tab} deadline=${deadlineSecs}s`);
  console.log(`peer-job: completion arrives as one message to tab ${replyTab}; no polling needed`);
}

function requireReplyTab(opts) {
  const tab = opts.replyTab ?? process.env.CODETERM_TAB_ID;
  if (!tab) throw new Refusal('peer jobs report completion to an agent tab; run inside Domios or pass --reply-tab');
  return tab;
}

async function dispatch(argv) {
  const verb = argv.shift();
  const stateDir = takeOption(argv, '--state-dir');
  if (!stateDir) throw new Refusal('dispatch requires --state-dir');
  const opts = parseDispatchArgs(verb, argv);
  const config = peersConfig();
  const replyTab = requireReplyTab(opts);
  if (verb === 'check-series') {
    const spec = parseSeriesSpec(JSON.parse(fs.readFileSync(opts.spec, 'utf8')));
    for (const base of spec.bases) resolveSha(base.sha);
    // The job checks out this runner's SHA, so one build.sh drives every base; scratch commits never touch the checkout.
    const runnerSha = resolveSha('HEAD');
    const peer = opts.peer === 'auto' ? await route(config, opts.os) : findPeer(config.peers, opts.peer);
    const specB64 = Buffer.from(JSON.stringify(spec)).toString('base64');
    launch({ config, peer, verb, sha: runnerSha, deadlineSecs: parseDuration(opts.deadline), replyTab, clone: false, stateDir,
      recordNames: [], runnerTail: { head: ['run-series'], rest: ['--spec-b64', specB64] } });
    return;
  }
  const sha = resolveSha(opts.sha ?? 'HEAD');
  const recordNames = [];
  if (verb === 'check-linux') {
    const receipt = path.join(stateDir, `check-linux-${sha}.json`);
    if (fs.existsSync(receipt) && !opts.force) {
      console.log(`check-linux: already dispatched for ${sha} (one per batch): ${JSON.stringify(JSON.parse(fs.readFileSync(receipt, 'utf8')))}`);
      console.log('check-linux: pass --force to dispatch again');
      return;
    }
    recordNames.push(`check-linux-${sha}`);
  }
  const peer = opts.peer === 'auto' ? await route(config, opts.os) : findPeer(config.peers, opts.peer);
  if (opts.os && peer.os !== opts.os) throw new Refusal(`peer ${peer.name} is ${peer.os}, not ${opts.os}`);
  const remoteArgs = verb === 'nextest' ? opts.remoteArgs : [sha, ...opts.remoteArgs];
  launch({ config, peer, verb, sha, deadlineSecs: parseDuration(opts.deadline), replyTab, clone: false, stateDir, recordNames,
    runnerTail: { head: ['run-job'], rest: ['--', VERBS[verb].remote, ...remoteArgs] } });
}

function setup(argv) {
  const stateDir = takeOption(argv, '--state-dir');
  const replyTab = requireReplyTab({ replyTab: takeOption(argv, '--reply-tab') });
  const ref = takeOption(argv, '--sha') ?? 'origin/develop';
  const install = argv.includes('--install-toolchain');
  if (install) argv.splice(argv.indexOf('--install-toolchain'), 1);
  if (!stateDir || argv.length !== 1) throw new Refusal('usage: ./build.sh peer-setup <peer> [--sha REF] [--install-toolchain] [--reply-tab TAB]');
  const config = peersConfig();
  const peer = findPeer(config.peers, argv[0]);
  const sha = resolveSha(ref);
  const rest = ['--status-title', config.statusTabTitle, ...(peer.shell ? ['--shell', peer.shell] : []),
    ...(install ? ['--install-toolchain', '1'] : [])];
  launch({ config, peer, verb: 'setup', sha, deadlineSecs: parseDuration(VERBS.setup.deadline), replyTab, clone: true,
    stateDir, recordNames: [], runnerTail: { head: ['setup-local'], rest } });
}

function jobArgs(argv) {
  const opts = {};
  while (argv.length && argv[0] !== '--') {
    const key = argv.shift().replace(/^--/, '');
    opts[key] = argv.shift();
  }
  argv.shift();
  for (const key of ['id', 'peer-name', 'verb', 'sha', 'reply-peer', 'reply-tab']) {
    if (!opts[key]) throw new Refusal(`peer job requires --${key}`);
  }
  return { opts, rest: argv };
}

function killTree(child) {
  if (process.platform === 'win32') spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true });
  else {
    try { process.kill(-child.pid, 'SIGKILL'); } catch { child.kill('SIGKILL'); }
  }
}

function releaseLock() {
  fs.rmSync(path.join(git(['rev-parse', '--git-common-dir']), LOCK_NAME), { recursive: true, force: true });
}

function sendEvent(opts, text) {
  process.stdout.write(`\n${text}`);
  const sent = cli(['send', '--require-agent', '--stdin', '--tab', opts['reply-tab'], '--mesh', opts['reply-peer']],
    { input: text, timeoutMs: SEND_TIMEOUT_MS });
  if (sent.status !== 0) console.error(`peer-job: completion event was not delivered: ${(sent.stderr || sent.stdout).trim()}`);
}

function jobLogPath(id) {
  const dir = path.resolve(git(['rev-parse', '--git-common-dir']), 'codeterm-peer-jobs');
  fs.mkdirSync(dir, { recursive: true });
  return path.join(dir, `${id}.log`);
}

function spawnBuild(args, log, deadlineAt) {
  return new Promise((resolve) => {
    const child = spawn('bash', ['./build.sh', ...args, '--wait-build'], {
      detached: process.platform !== 'win32', stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
    });
    let timedOut = false;
    let output = '';
    const deadline = setTimeout(() => { timedOut = true; killTree(child); }, Math.max(0, deadlineAt - Date.now()));
    for (const stream of [child.stdout, child.stderr]) {
      stream.on('data', (chunk) => { output += chunk; process.stdout.write(chunk); log.write(chunk); });
    }
    child.on('close', (code) => {
      clearTimeout(deadline);
      resolve({ code: code ?? 1, output, timedOut });
    });
  });
}

function finishJob(opts, log, started, verb, exitCode, timedOut, parsed, logPath) {
  log.end(() => {
    // The event is the caller's cue to dispatch again, and send --require-agent can block, so free the checkout first.
    releaseLock();
    sendEvent(opts, jobEvent({ jobId: opts.id, peer: opts['peer-name'], verb, sha: opts.sha,
      outcome: outcomeOf(exitCode, timedOut), exitCode, wallSecs: Math.round((Date.now() - started) / 1000), parsed: parsed(), logPath }));
    process.exitCode = exitCode;
  });
}

async function runJob(argv) {
  const { opts, rest } = jobArgs(argv);
  const deadlineSecs = Number(opts['deadline-secs']);
  if (!rest.length || !(deadlineSecs > 0)) throw new Refusal('peer job requires --deadline-secs and a build verb after --');
  const logPath = jobLogPath(opts.id);
  const log = fs.createWriteStream(logPath);
  const started = Date.now();
  const run = await spawnBuild(rest, log, started + deadlineSecs * 1000);
  finishJob(opts, log, started, opts.verb, run.code, run.timedOut, () => parseJobLog(fs.readFileSync(logPath, 'utf8')), logPath);
}

async function runSeriesJob(argv) {
  const { opts } = jobArgs(argv);
  const deadlineSecs = Number(opts['deadline-secs']);
  if (!opts['spec-b64'] || !(deadlineSecs > 0)) throw new Refusal('series job requires --spec-b64 and --deadline-secs');
  const spec = parseSeriesSpec(JSON.parse(Buffer.from(opts['spec-b64'], 'base64').toString('utf8')));
  const logPath = jobLogPath(opts.id);
  const resultsPath = logPath.replace(/\.log$/, '.series.json');
  const log = fs.createWriteStream(logPath);
  const started = Date.now();
  const results = [];
  let outcome = { results, expected: 0, stopped: null };
  let crashed = null;
  try {
    for (const base of spec.bases) {
      if (!git(['cat-file', '-e', `${base.sha}^{commit}`], { allowFail: true, status: true })) {
        git(['-c', 'http.lowSpeedLimit=1000', '-c', 'http.lowSpeedTime=60', 'fetch', '--quiet', '--no-tags', 'origin', base.sha]);
      }
    }
    outcome = await runSeries({
      spec, git, results, runCheck: (sha) => spawnBuild(['check-queue', sha], log, started + deadlineSecs * 1000),
      onRun: () => fs.writeFileSync(resultsPath, `${JSON.stringify({ partial: true, results }, null, 2)}\n`),
    });
  } catch (error) {
    crashed = error.message;
  }
  const own = new Set(outcome.results.flatMap((r) => Object.values(r.runs).map((run) => run.gatewayJob)).filter(Boolean));
  const gatewayDir = gatewayDirOf(outcome.results);
  const state = gatewayDir ? spawnSync(process.execPath, [path.join(ROOT, 'scripts/build-gateway-state.mjs'), 'status', '--dir', gatewayDir, '--limit', '200'],
    { encoding: 'utf8', windowsHide: true }) : null;
  const others = state?.status === 0 ? otherGatewayJobs(JSON.parse(state.stdout), own, started) : ['unknown'];
  const stopped = crashed ? `error:${crashed.slice(0, 200)}` : outcome.stopped;
  fs.writeFileSync(resultsPath, `${JSON.stringify({ ...outcome, stopped, other_gateway_jobs: others }, null, 2)}\n`);
  const summary = seriesSummary(outcome.results, { expected: outcome.expected || spec.bases.reduce((n, b) => n + b.areas.length, 0), stopped, resultsPath, otherGatewayJobs: others });
  const timedOut = Boolean(stopped?.startsWith('deadline:'));
  finishJob(opts, log, started, 'check-series', stopped ? (timedOut ? 124 : 1) : 0, timedOut, () => ({ summary, details: [], gatewayLog: null }), logPath);
}

function ensureStatusTab(title, shell) {
  const list = cli(['tab', 'list', '--json']);
  const existing = list.status === 0 ? (parseJson(list.stdout, 'tab list').tabs ?? []).find((t) => t.title === title) : null;
  if (existing) return existing.id;
  const args = ['tab', 'new', '--json', '--no-team', '--purpose', 'shell', '--title', title,
    '--description', 'Peer build gateway status probe', '--cwd', process.cwd()];
  if (shell) args.push('--shell', shell);
  const created = cli(args, { timeoutMs: 60000 });
  if (created.status !== 0) return null;
  const tab = parseJson(created.stdout, 'tab new');
  return tab.tab_id ?? tab.id ?? null;
}

function setupLocal(argv) {
  const { opts } = jobArgs(argv);
  const started = Date.now();
  const logPath = jobLogPath(opts.id);
  const toolchain = spawnSync('bash', ['./build.sh', 'peer-toolchain', ...(opts['install-toolchain'] ? ['--install'] : [])],
    { encoding: 'utf8', timeout: opts['install-toolchain'] ? 900000 : 300000, windowsHide: true });
  const output = `${toolchain.stdout ?? ''}${toolchain.stderr ?? ''}`;
  fs.writeFileSync(logPath, output);
  const statusTab = ensureStatusTab(opts['status-title'], opts.shell);
  const parsed = parseJobLog(output);
  parsed.summary.push(`peer-setup: checkout=${process.cwd()} head=${git(['rev-parse', 'HEAD'])} push=disabled status-tab=${statusTab ?? 'missing'}`);
  const exitCode = toolchain.status === 0 && statusTab ? 0 : 1;
  releaseLock();
  sendEvent(opts, jobEvent({ jobId: opts.id, peer: opts['peer-name'], verb: 'setup', sha: opts.sha,
    outcome: outcomeOf(exitCode, Boolean(toolchain.error)), exitCode, wallSecs: Math.round((Date.now() - started) / 1000), parsed, logPath }));
  process.exitCode = exitCode;
}

function status(argv) {
  const gatewayDir = takeOption(argv, '--gateway-dir');
  if (!gatewayDir) throw new Refusal('status requires --gateway-dir');
  const state = spawnSync(process.execPath, [path.join(ROOT, 'scripts/build-gateway-state.mjs'), 'status', '--dir', gatewayDir, '--limit', '1'],
    { encoding: 'utf8', windowsHide: true });
  const gateway = fs.existsSync(gatewayDir) && state.status === 0 ? JSON.parse(state.stdout) : { state: 'idle', queue: [], recent: [] };
  const lock = fs.existsSync(path.join(git(['rev-parse', '--git-common-dir']), LOCK_NAME));
  console.log(`${STATUS_MARKER}${JSON.stringify({ os: process.platform, ...summarizeGateway(gateway, lock, git(['rev-parse', 'HEAD'])) })}`);
}

async function main() {
  const [command, ...argv] = process.argv.slice(2);
  if (command === 'dispatch') await dispatch(argv);
  else if (command === 'setup') setup(argv);
  else if (command === 'run-job') await runJob(argv);
  else if (command === 'run-series') await runSeriesJob(argv);
  else if (command === 'setup-local') setupLocal(argv);
  else if (command === 'status') status(argv);
  else if (command === 'route') {
    const os = takeOption(argv, '--os');
    await route(peersConfig(), os);
  } else throw new Refusal('usage: build-peer.mjs dispatch|setup|run-job|run-series|setup-local|status|route');
}

main().catch((error) => {
  console.error(`ERROR: ${error.message}`);
  process.exitCode = error instanceof Refusal ? error.code : 1;
});
