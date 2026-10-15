// Caller: scripts build-peer.mjs, build-peer.test.mjs; skill build
export const STATUS_MARKER = 'PEER_STATUS ';
export const LOCK_NAME = 'codeterm-peer-job.lock';
export const STALE_LOCK_MINUTES = 240;
const SHELL_SAFE_PATH = /^[A-Za-z0-9_/.~$-]+$/;
const NAME = /^[A-Za-z0-9_.-]+$/;
const SHA = /^[0-9a-f]{40}$/;

export const VERBS = {
  'check-queue': { deadline: '45m', os: null, remote: 'check-queue' },
  nextest: { deadline: '30m', os: null, remote: 'nextest' },
  'check-linux': { deadline: '45m', os: 'linux', remote: 'check-workspace' },
  setup: { deadline: '20m', os: null, remote: null },
};

export function parseDuration(text) {
  const match = /^(\d+)([smh])$/.exec(String(text));
  if (!match || Number(match[1]) <= 0) throw new Error(`invalid duration '${text}' (use 30s, 45m or 2h)`);
  return Number(match[1]) * { s: 1, m: 60, h: 3600 }[match[2]];
}

export function loadPeers(config, env = {}) {
  const enabled = new Set(String(env.CODETERM_PEERS_ENABLE ?? '').split(',').map((s) => s.trim().toLowerCase()).filter(Boolean));
  const peers = Object.entries(config.peers ?? {}).map(([name, peer]) => {
    if (!NAME.test(name)) throw new Error(`peer name '${name}' is not a plain name`);
    if (!['windows', 'macos', 'linux'].includes(peer.os)) throw new Error(`peer ${name} has unknown os '${peer.os}'`);
    if (!SHELL_SAFE_PATH.test(peer.checkout ?? '')) throw new Error(`peer ${name} checkout '${peer.checkout}' is not a plain path`);
    const extraPath = peer.path ?? [];
    if (!Array.isArray(extraPath) || !extraPath.every((dir) => SHELL_SAFE_PATH.test(dir))) throw new Error(`peer ${name} path must be plain directories`);
    return {
      name,
      os: peer.os,
      checkout: peer.checkout,
      shell: peer.shell ?? null,
      path: extraPath,
      routable: peer.routable !== false || enabled.has(name.toLowerCase()),
      note: peer.note ?? null,
    };
  });
  return { repository: config.repository, statusTabTitle: config.status_tab_title ?? 'Buildpeer', peers };
}

export function findPeer(peers, name) {
  const peer = peers.find((p) => p.name.toLowerCase() === String(name).toLowerCase());
  if (!peer) throw new Error(`unknown peer '${name}'; known: ${peers.map((p) => p.name).join(', ')}`);
  if (!peer.routable) throw new Error(`peer ${peer.name} is not routable${peer.note ? `: ${peer.note}` : ''}`);
  return peer;
}

const VALUE_FLAGS = new Set(['--peer', '--os', '--deadline', '--reply-tab', '--sha', '--max-errors', '--max-warnings']);
const PASS_FLAGS = new Set(['--max-errors', '--max-warnings']);
const PASS_SWITCHES = new Set(['--touched-only', '--all']);

export function parseDispatchArgs(verb, argv) {
  if (!VERBS[verb]) throw new Error(`verb '${verb}' has no peer path`);
  const opts = { verb, peer: verb === 'check-linux' ? 'auto' : null, os: VERBS[verb].os, deadline: VERBS[verb].deadline,
    replyTab: null, sha: null, force: false, lane: false, positionals: [], passthrough: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (VALUE_FLAGS.has(arg)) {
      const value = argv[i + 1];
      if (value === undefined || value.startsWith('--')) throw new Error(`${arg} requires a value`);
      i += 1;
      if (PASS_FLAGS.has(arg) && verb === 'check-queue') opts.passthrough.push(arg, value);
      else if (arg === '--peer') opts.peer = value;
      else if (arg === '--os') opts.os = value;
      else if (arg === '--deadline') opts.deadline = value;
      else if (arg === '--reply-tab') opts.replyTab = value;
      else if (arg === '--sha') opts.sha = value;
      else throw new Error(`${arg} is not valid for ${verb}`);
    } else if (arg === '--lane' && verb === 'check-queue') { opts.lane = true; opts.passthrough.push(arg); }
    else if (arg === '--force') opts.force = true;
    else if (PASS_SWITCHES.has(arg) && verb === 'check-queue') opts.passthrough.push(arg);
    else if (arg === '--wait-build') continue;
    else if (arg.startsWith('--')) throw new Error(`unknown option ${arg} for peer ${verb}`);
    else opts.positionals.push(arg);
  }
  if (!opts.peer) throw new Error(`${verb} peer dispatch requires --peer <name|auto>`);
  if (opts.os && !['windows', 'macos', 'linux'].includes(opts.os)) throw new Error(`unknown --os '${opts.os}'`);
  if (opts.lane && opts.passthrough.includes('--all')) throw new Error('--lane and --all are exclusive');
  if (verb === 'nextest') {
    if (opts.positionals.length < 1 || opts.positionals.length > 2) throw new Error('usage: ./build.sh nextest <scope> [filterset] --peer <name|auto> [--sha SHA]');
    opts.remoteArgs = [...opts.positionals];
  } else {
    if (opts.positionals.length > 1) throw new Error(`usage: ./build.sh ${verb} [sha] --peer <name|auto>`);
    if (opts.sha && opts.positionals.length) throw new Error('give the SHA once');
    opts.sha = opts.positionals[0] ?? opts.sha;
    opts.remoteArgs = [...opts.passthrough];
  }
  parseDuration(opts.deadline);
  return opts;
}

function loadOf(status) {
  return (status.gateway.active ? 1 : 0) + Number(status.gateway.queue ?? 0) + (status.job_lock ? 1 : 0);
}

// Candidates are { peer, status|null, error|null }; only an idle, reachable peer of the right OS wins.
export function pickPeer(candidates, os) {
  const considered = candidates.filter((c) => c.peer.routable && (!os || c.peer.os === os));
  if (considered.length === 0) return { error: 'no_peer', detail: `no routable peer${os ? ` with os=${os}` : ''}` };
  const idle = considered
    .filter((c) => c.status && c.status.gateway.state === 'idle' && !c.status.job_lock)
    .sort((a, b) => loadOf(a.status) - loadOf(b.status)
      || String(a.status.gateway.last_ended_at ?? '').localeCompare(String(b.status.gateway.last_ended_at ?? ''))
      || a.peer.name.localeCompare(b.peer.name));
  const detail = considered.map((c) => `${c.peer.name}=${c.status ? `${c.status.gateway.state}/load${loadOf(c.status)}` : `unknown(${c.error ?? 'no status'})`}`).join(' ');
  if (idle.length === 0) return { error: 'busy', detail };
  return { peer: idle[0].peer, detail };
}

export function parseStatusOutput(output) {
  const line = String(output ?? '').split(/\r?\n/).map((l) => l.trim()).filter((l) => l.startsWith(STATUS_MARKER)).pop();
  if (!line) return null;
  try {
    const status = JSON.parse(line.slice(STATUS_MARKER.length));
    return status && status.gateway && typeof status.gateway.state === 'string' ? status : null;
  } catch {
    return null;
  }
}

export function summarizeGateway(gateway, jobLock, head) {
  return {
    gateway: {
      state: gateway.state,
      active: Boolean(gateway.active),
      queue: (gateway.queue ?? []).length,
      job: gateway.job ?? null,
      last_ended_at: gateway.recent?.[0]?.ended_at ?? null,
    },
    job_lock: Boolean(jobLock),
    head: head ?? null,
  };
}

export function shellQuote(value) {
  return `'${String(value).replace(/'/g, `'\\''`)}'`;
}

function checkoutWord(checkout) {
  return `"${checkout}"`;
}

// Mesh shells are non-login, so a peer whose node or cargo lives outside the default PATH lists it in peers.json.
export function pathPrefix(peer) {
  return peer.path?.length ? `export PATH="${peer.path.join(':')}:$PATH"; ` : '';
}

export function eventHeader({ jobId, peer, verb, sha }) {
  return `[peer-job ${jobId}] peer=${peer} verb=${verb} sha=${sha}`;
}

// One POSIX command line for the peer's shell: lock, fetch the SHA, then the job runner from that SHA.
export function remoteCommand({ peer, repository, jobId, verb, sha, clone, runner, reply }) {
  if (!SHA.test(sha)) throw new Error(`remote commands need a full SHA, got '${sha}'`);
  if (!NAME.test(jobId) || !NAME.test(reply.tab) || !NAME.test(reply.peer)) throw new Error('job id and reply route must be plain names');
  const header = eventHeader({ jobId, peer: peer.name, verb, sha });
  const send = `codeterm send --require-agent --tab ${reply.tab} --mesh ${reply.peer}`;
  const lock = `.git/${LOCK_NAME}`;
  const parts = [
    `${pathPrefix(peer)}ct_rc=0; ct_stage=start`,
    `ct_step() { ct_stage=$1; shift; "$@" || { ct_rc=$?; return 1; }; }`,
    `ct_fail() { rm -rf ${lock}; ${send} "${header} outcome=$1 exit=$ct_rc stage=$ct_stage"; }`,
  ];
  if (clone) {
    parts.push(`ct_clone() { [ -d ${checkoutWord(`${peer.checkout}/.git`)} ] || { mkdir -p ${checkoutWord(peer.checkout.replace(/\/[^/]+$/, ''))} && git -c http.lowSpeedLimit=1000 -c http.lowSpeedTime=60 clone --quiet --no-tags ${shellQuote(repository)} ${checkoutWord(peer.checkout)}; }; }`);
  }
  parts.push(
    `if ! { ${clone ? 'ct_step clone ct_clone && ' : ''}ct_step checkout_dir cd ${checkoutWord(peer.checkout)}; }; then ${send} "${header} outcome=not_setup exit=$ct_rc stage=$ct_stage"`
    + `; elif ! { mkdir ${lock} 2>/dev/null || { [ -n "$(find ${lock} -maxdepth 0 -mmin +${STALE_LOCK_MINUTES} 2>/dev/null)" ] && rm -rf ${lock} && mkdir ${lock}; }; }; then ${send} "${header} outcome=busy exit=75 stage=peer_lock"`
    + `; elif ! ct_step push_url git remote set-url --push origin fetch-only.invalid; then ct_fail bootstrap_failed`
    + `; elif ! ct_step detach git checkout --quiet --detach; then ct_fail bootstrap_failed`
    + `; elif ! ct_step fetch git -c http.lowSpeedLimit=1000 -c http.lowSpeedTime=60 fetch --quiet --no-tags origin +refs/heads/develop:refs/heads/develop ${sha}; then ct_fail bootstrap_failed`
    + `; elif ! ct_step checkout git checkout --quiet --detach ${sha}; then ct_fail bootstrap_failed`
    + `; elif ! ct_step runner_missing test -f scripts/build-peer.mjs; then ct_fail bootstrap_failed`
    + `; elif ! ct_step node_missing command -v node; then ct_fail bootstrap_failed`
    + `; else node scripts/build-peer.mjs ${runner.map(shellQuote).join(' ')}; fi`,
    'exit',
  );
  return parts.join('; ');
}

const SUMMARY_PATTERNS = [
  /^check-queue: /,
  /^check-workspace: /,
  /^errors=\d+/,
  /^warnings=\d+ shown=/,
  /^warnings crate=/,
  /^peer-toolchain: /,
  /^Summary \[/,
  /^test result: /,
  /^DEADLOCK \/ HANG: /,
];
const DETAIL_PATTERNS = [/^\S+:\d+:\d+: error/, /^(sccache: )?error(\[E\d+\])?: /, /^(FAIL|TIMEOUT|SIGSEGV|ABORT) \[/];
const MAX_DETAIL_LINES = 12;

export function stripAnsi(text) {
  return String(text).replace(/\x1b\[[0-9;?]*[A-Za-z]/g, '').replace(/\r/g, '');
}

export function parseJobLog(text) {
  const summary = [];
  const details = [];
  let gatewayLog = null;
  for (const raw of stripAnsi(text).split('\n')) {
    const line = raw.trim();
    const gateway = /^Build gateway job \S+ log: (.+)$/.exec(line);
    if (gateway) gatewayLog = gateway[1];
    if (SUMMARY_PATTERNS.some((re) => re.test(line))) summary.push(line);
    else if (details.length < MAX_DETAIL_LINES && !details.includes(line) && DETAIL_PATTERNS.some((re) => re.test(line))) details.push(line);
  }
  return { summary, details, gatewayLog };
}

export function outcomeOf(exitCode, timedOut) {
  if (timedOut) return 'deadline';
  if (exitCode === 0) return 'passed';
  if (exitCode === 75) return 'busy';
  return 'failed';
}

export function jobEvent({ jobId, peer, verb, sha, outcome, exitCode, wallSecs, parsed, logPath }) {
  const lines = [`${eventHeader({ jobId, peer, verb, sha })} outcome=${outcome} exit=${exitCode ?? 'none'} wall=${wallSecs}s`];
  lines.push(...parsed.summary);
  if (parsed.summary.length === 0) lines.push('no summary line found; read the remote log');
  lines.push(...parsed.details);
  lines.push(`remote-log=${logPath}${parsed.gatewayLog ? ` gateway-log=${parsed.gatewayLog}` : ''}`);
  return `${lines.join('\n')}\n`;
}

export function newJobId(now = new Date(), random = Math.random) {
  const stamp = now.toISOString().replace(/[-:]/g, '').replace(/\.\d+Z$/, 'Z');
  return `peer-${stamp}-${Math.floor(random() * 0xffff).toString(16).padStart(4, '0')}`;
}
