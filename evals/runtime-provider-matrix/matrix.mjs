import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { performance } from 'node:perf_hooks';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '../..');
const provider = process.env.PROVIDER;
const cap = Number(process.env.MATRIX_DEADLINE_SECS || 60) * 1000;
const runId = `matrix-${Date.now()}-${process.pid}`;
const out = join(root, 'target-check/evals/runtime-provider-matrix', runId);
mkdirSync(out, { recursive: true });
const prompts = JSON.parse(readFileSync(join(here, 'fixtures/prompts.json'), 'utf8'));
const resumeFixture = process.env.MATRIX_RESUME_FIXTURE
  ? JSON.parse(readFileSync(process.env.MATRIX_RESUME_FIXTURE, 'utf8')) : null;
const codeword = resumeFixture?.codeword || `opaque${process.pid}${Date.now()}`;
const rows = [];
const owned = { tabs: [], workers: [], crons: [], workspace: null, plan: null, owner: null };
let sequence = 0;

async function bridge(mode, ...args) {
  const started = performance.now();
  return new Promise((resolve, reject) => {
    const child = spawn('bash', [join(here, 'bridge.sh'), mode, ...args], {
      stdio: ['ignore', 'pipe', 'pipe'], signal: AbortSignal.timeout(cap),
    });
    let stdout = '', stderr = '';
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.on('error', reject);
    child.on('close', exit => {
      const record = { mode, args, exit, stdout, stderr, ms: performance.now() - started };
      writeFileSync(join(out, `${++sequence}.json`), JSON.stringify(record, null, 2));
      if (exit !== 0) return reject(new Error(JSON.stringify(record)));
      try { resolve(JSON.parse(stdout)); } catch { resolve(stdout.trim()); }
    });
  });
}
const cli = (...args) => bridge('cli', ...args, '--json');
const get = path => bridge('get', path);
const post = (path, body) => bridge('post', path, JSON.stringify(body));
const first = value => Array.isArray(value) ? value[0] : value;
const data = value => value.data || value;
const assert = (condition, evidence) => { if (!condition) throw new Error(JSON.stringify(evidence)); };
const inspect = async id => first(await cli('tab', 'inspect', id));
const history = async id => (await cli('tab', 'chat', '--tab', id)).messages;
async function poll(read, predicate, label) {
  const deadline = performance.now() + cap;
  let evidence;
  do {
    evidence = await read();
    if (predicate(evidence)) return evidence;
  } while (performance.now() < deadline);
  throw new Error(`${label}: deadline; last evidence=${JSON.stringify(evidence)}`);
}
async function row(id, name, action) {
  try {
    const evidence = await action();
    rows.push({ id, name, status: 'PASS', evidence });
  } catch (error) {
    rows.push({ id, name, status: 'FAIL', evidence: error.message });
  }
  const result = rows.at(-1);
  console.log(`${result.status} ${id} ${name}: ${JSON.stringify(result.evidence)}`);
  writeFileSync(join(out, 'results.json'), JSON.stringify(rows, null, 2));
}
async function spawnTab(title, options = []) {
  const response = data(await cli('agent', 'spawn', provider, '--workspace', owned.workspace,
    '--title', title, '--description', 'Disposable runtime matrix probe', '--non-user-facing', ...options));
  const id = response.tab_id || response.id;
  assert(id, response);
  owned.tabs.push(id);
  const tab = await inspect(id);
  assert(tab.workspace_id === owned.workspace && !tab.protected, tab);
  assert(tab.backing?.kind === 'chat_backend' && tab.agent_session_id, tab);
  return id;
}
function padFile(text) {
  const name = `${runId}-${++sequence}.md`;
  const path = join(process.env.MATRIX_PAD, name);
  writeFileSync(path, text);
  return path;
}
async function ask(id, text) {
  assert(owned.tabs.includes(id), { unowned: id });
  const before = new Set((await history(id)).map(message => message.id));
  await cli('send', '--tab', id, '--from-file', padFile(text), '--require-agent');
  const messages = await poll(() => history(id), messages => messages.some(message =>
    message.type === 'assistant' && !before.has(message.id)), 'new assistant reply');
  await poll(() => inspect(id), tab => tab.agent_state?.state === 'idle', 'turn idle');
  return messages.filter(message => message.type === 'assistant' && !before.has(message.id))
    .map(message => message.content).join('\n');
}
async function identity(id, expected) {
  const answer = await ask(id, prompts.identity);
  assert(expected.every(token => answer.toLowerCase().includes(token.toLowerCase())), { expected, answer });
  return { tab: id, answer };
}

let bare, orchestrator, planner, worker, session, models, nativeBefore, restartObserved = false;
try {
  const health = await get('/api/health');
  console.log(`EVIDENCE build=${health.build_sha} provider=${provider} output=${out}`);
  const providers = await cli('agent', 'providers');
  const descriptor = providers.find(item => item.id === provider);
  assert(descriptor?.agent_runtime || descriptor?.agentRuntime, { provider, reason: 'agentRuntime capability required' });
  models = await cli('agent', 'models', provider);
  const ws = resumeFixture?.workspace || await cli('workspace', 'create', runId, '--provision');
  assert(ws.id && ws.provisioned && !ws.protected, ws);
  owned.workspace = ws.id;
  await row('1', 'system_at_start_and_cli_send', async () => {
    if (resumeFixture) {
      bare = resumeFixture.tab;
      const tab = await inspect(bare);
      assert(tab.workspace_id === ws.id && !tab.protected, { reason: 'resume fixture not owned', tab });
      owned.tabs.push(bare);
      session = resumeFixture.session;
      const messages = await history(bare);
      assert(messages.some(item => item.id === resumeFixture.assistant_id && item.type === 'assistant'
        && item.content === codeword), { reason: 'pre-restart exact assistant evidence missing', messages });
      nativeBefore = { id: bare, session, messages };
      return { provenance: resumeFixture.provenance, assistant: resumeFixture.assistant_id, answer: codeword };
    }
    bare = await spawnTab('Calibration', ['--system', `The calibration fact is ${codeword}. Keep answers brief.`]);
    const answer = await ask(bare, prompts.fact);
    assert(answer.includes(codeword), { answer, codeword });
    session = (await inspect(bare)).agent_session_id;
    nativeBefore = { id: bare, session, messages: await history(bare) };
    return { tab: bare, session, answer };
  });
  await row('2', 'daemon_restart_system_resume', async () => {
    assert(nativeBefore, { reason: 'scenario 1 failed' });
    if (resumeFixture) {
      const restored = await inspect(bare);
      assert(health.build_sha !== resumeFixture.build && restored.session_binding_nonce !== resumeFixture.nonce
        && restored.agent_session_id === session, { build: health.build_sha, session: restored.agent_session_id, nonce: restored.session_binding_nonce });
      restartObserved = true;
      const answer = await ask(bare, prompts.fact);
      assert(answer.includes(codeword), { answer, codeword });
      return { build: health.build_sha, restoredSession: session, answer };
    }
    const checkpoint = process.env.MATRIX_RESTART_CHECKPOINT;
    assert(checkpoint, { reason: 'Shared DEV restart requires operator; set MATRIX_RESTART_CHECKPOINT to a new marker path.' });
    assert(!existsSync(checkpoint), { reason: 'restart marker already exists', checkpoint });
    const oldPid = await bridge('pid');
    writeFileSync(`${checkpoint}.ready`, JSON.stringify({ workspace: ws.id, nativeBefore, oldPid }));
    console.log(`CHECKPOINT operator restart DEV then create ${checkpoint}; deadline=${cap}ms`);
    await poll(async () => existsSync(checkpoint), Boolean, 'operator restart marker');
    const newPid = await bridge('pid');
    assert(oldPid && newPid && oldPid !== newPid, { oldPid, newPid });
    restartObserved = true;
    const answer = await ask(bare, prompts.fact);
    assert(answer.includes(codeword), { answer, codeword });
    return { oldPid, newPid, answer };
  });
  await row('3', 'restored_native_and_legacy_history', async () => {
    assert(restartObserved, { reason: 'no observed daemon restart' });
    const tab = await inspect(bare);
    const messages = await history(bare);
    assert(tab.backing?.kind === 'chat_backend' && tab.agent_session_id === session && messages.length > 0,
      { backing: tab.backing, session: tab.agent_session_id, count: messages.length });
    assert(process.env.MATRIX_LEGACY_TAB, { reason: 'legacy PTY fixture missing; requires an owned pre-runtime tab before operator restart' });
    const legacy = await inspect(process.env.MATRIX_LEGACY_TAB);
    assert(legacy.workspace_id === owned.workspace && !legacy.protected, { reason: 'legacy fixture must be in owned workspace' });
    assert(legacy.backing?.kind === 'chat_backend' && legacy.agent_session_id === process.env.MATRIX_LEGACY_SESSION,
      { backing: legacy.backing, session: legacy.agent_session_id });
    assert((await history(legacy.id)).length > 0, { reason: 'legacy history empty' });
    return { native: tab.id, legacy: legacy.id, count: messages.length };
  });
  await row('4', 'model_cli_self_and_peer', async () => {
    assert(bare && models.length >= 2, { reason: 'two provider models and calibration tab required' });
    const original = (await inspect(bare)).agent_session_model;
    const alternate = models.find(model => model.id !== original).id;
    const confirmations = [];
    async function confirm(id, model) {
      await ask(id, 'Reply only with READY. Do not call tools or send messages.');
      const tabs = await cli('tab', 'list', '--workspace', owned.workspace);
      const list = Array.isArray(tabs) ? tabs : tabs.tabs;
      const tab = list.find(tab => tab.id === id || tab.tab_id === id);
      assert(tab?.agent_session_model === model || tab?.model === model, { expected: model, tab });
      confirmations.push({ id, model, tab });
    }
    await cli('agent', 'model', alternate, '--tab', bare);
    await confirm(bare, alternate);
    const command = `source ${JSON.stringify(join(root, 'evals/_lib/lib.sh'))}; dc --as-tab "$CODETERM_TAB_ID" agent model ${JSON.stringify(original)} --json`;
    const answer = await ask(bare, `Run this exact shell command in your own turn: ${command}. Then reply with its result. Do not send messages.`);
    assert(answer.length > 0, { answer });
    await confirm(bare, original);
    const peer = await spawnTab('Peer');
    await ask(peer, `Run this shell command: source ${JSON.stringify(join(root, 'evals/_lib/lib.sh'))}; dc --as-tab "$CODETERM_TAB_ID" agent model ${JSON.stringify(alternate)} --tab ${bare} --json. Reply with its result; do not send messages.`);
    await confirm(bare, alternate);
    return confirmations;
  });
  await row('5', 'worker_dispatch_ack_and_identity', async () => {
    orchestrator = await spawnTab('Conductor', ['--role', 'Orchestrator']);
    owned.owner = orchestrator;
    const planResponse = data(await cli('--as-tab', orchestrator, 'plan', 'create', '--title', runId, '--project', ws.cwd));
    owned.plan = planResponse.plan_id || planResponse.id || planResponse.plan?.id;
    assert(owned.plan, planResponse);
    await cli('--as-tab', orchestrator, 'plan', 'goal', 'set', owned.plan, 'Runtime identity calibration', '--vector', 'Worker ACK and opaque identity round-trip');
    const taskResponse = data(await cli('--as-tab', orchestrator, 'plan', 'task', 'add', owned.plan,
      '--name', 'Identity calibration', '--executor-role', 'Calibration executor', '--personality', 'Prism',
      '--role-guidance', `Your opaque role-guidance codeword is ${codeword}. Answer questions briefly.`,
      '--body-file', padFile('Acknowledge the assigned task with agent ack, then await questions. Do not edit files or report completion.'),
      '--criterion', 'Identity round-trip', '--test-plan', 'Ask the assigned identity', '--correctness', 'Instructions arrive at runtime start',
      '--execution-intent', 'non_repository'));
    const taskId = taskResponse.task_id || taskResponse.id || taskResponse.task?.id;
    assert(taskId, taskResponse);
    await cli('--as-tab', orchestrator, 'plan', 'mode', owned.plan, 'approved');
    const dispatched = data(await cli('--as-tab', orchestrator, 'plan', 'task', 'dispatch', owned.plan, taskId,
      '--agent', provider, '--execution-intent', 'read_only', '--workspace', ws.id, '--cwd', ws.cwd));
    worker = dispatched.tab_id || dispatched.worker_tab_id;
    assert(worker, dispatched);
    owned.tabs.push(worker);
    const task = await poll(() => cli('plan', 'task', 'show', owned.plan, taskId), result =>
      (result.task || result).status === 'in_progress', 'durable Worker ACK');
    return { task, identity: await identity(worker, ['Prism', 'Calibration executor', codeword]) };
  });
  await row('6', 'planner_identity', async () => {
    assert(owned.plan && orchestrator, { reason: 'owned plan required' });
    planner = await spawnTab('Surveyor', ['--role', 'Planner', '--plan', owned.plan, '--parent-tab', orchestrator]);
    return identity(planner, ['Planner']);
  });
  await row('7', 'orchestrator_and_general_identity', async () => {
    assert(orchestrator, { reason: 'owned Orchestrator required' });
    const conductor = await identity(orchestrator, ['Orchestrator']);
    assert(process.env.MATRIX_GENERAL_TAB, { conductor, reason: 'No workspace-scoped General Agent spawn surface; shared protected General tab must not be used.' });
    const general = await inspect(process.env.MATRIX_GENERAL_TAB);
    assert(general.workspace_id === ws.id && !general.protected, { reason: 'General fixture not owned' });
    owned.tabs.push(general.id);
    return { conductor, general: await identity(general.id, ['General Agent']) };
  });
  await row('8', 'compaction_identity', async () => {
    assert(worker && planner && orchestrator, { reason: 'all three owned role tabs required' });
    const evidence = [];
    for (const [id, expected] of [[worker, ['Prism', 'Calibration executor', codeword]], [planner, ['Planner']], [orchestrator, ['Orchestrator']]]) {
      const before = new Set((await history(id)).map(item => item.id));
      await cli('agent', 'compact', '--tab', id);
      const compacted = await poll(() => history(id), messages => messages.some(item =>
        !before.has(item.id) && /compact/i.test(item.type)), 'completed compaction item');
      evidence.push({ compacted: compacted.filter(item => !before.has(item.id)), identity: await identity(id, expected) });
    }
    return evidence;
  });
  await row('9', 'restart_resume_and_fresh_identity', async () => {
    assert(worker, { reason: 'owned Worker required' });
    const before = await inspect(worker);
    const resumed = await cli('agent', 'restart', '--tab', worker);
    const after = await inspect(worker);
    assert(after.agent_session_id === before.agent_session_id, { resumed, before: before.agent_session_id, after: after.agent_session_id });
    const resumeIdentity = await identity(worker, ['Prism', 'Calibration executor', codeword]);
    const fresh = await cli('agent', 'restart', '--tab', worker, '--fresh', '--force');
    const renewed = await inspect(worker);
    assert(renewed.agent_session_id && renewed.agent_session_id !== before.agent_session_id, { fresh, session: renewed.agent_session_id });
    return { resumed, fresh, resumeIdentity, freshIdentity: await identity(worker, ['Prism', 'Calibration executor', codeword]) };
  });
  await row('10', 'interrupt_long_turn_then_idle', async () => {
    assert(bare, { reason: 'owned calibration tab required' });
    await cli('send', '--tab', bare, '--from-file', padFile(prompts.long), '--require-agent');
    const working = await poll(() => inspect(bare), tab => tab.agent_state?.state === 'working', 'active long turn');
    const stopped = await cli('tab', 'stop', '--tab', bare);
    const idle = await poll(() => inspect(bare), tab => tab.agent_state?.state === 'idle', 'idle after interrupt');
    assert(idle.agent_session_id === working.agent_session_id, { stopped, working, idle });
    return { stopped, session: idle.agent_session_id, state: idle.agent_state };
  });
  await row('11', 'cron_hidden_worker_and_dreamer_reports', async () => {
    assert(orchestrator, { reason: 'owned Team required' });
    const task = `Report calibration ${codeword} using cron complete with your own run id. Do not change memory, edit files, or send messages.`;
    const job = data(await cli('--as-tab', orchestrator, 'cron', 'create', 'Calibration', '--schedule', 'in 1h', '--once',
      '--task', task, '--agent', provider, '--workspace', ws.id, '--cwd', ws.cwd, '--timeout-secs', '60'));
    const jobId = job.id || job.job?.id;
    assert(jobId, job);
    owned.crons.push(jobId);
    await cli('cron', 'trigger', jobId);
    const runs = await poll(() => cli('cron', 'history', jobId), result => JSON.stringify(result).includes(codeword), 'persisted cron report');
    const hidden = data(await cli('mem', 'worker', 'spawn', '--agent', provider, '--intent', runId,
      '--task', `Do not read or modify memory. Report ${codeword} through mem worker report, then stop.`, '--parent', orchestrator));
    const hiddenId = hidden.id || hidden.worker_id || hidden.worker?.id;
    assert(hiddenId, hidden);
    owned.workers.push(hiddenId);
    const report = await poll(() => cli('mem', 'worker', 'get', hiddenId), result => JSON.stringify(result).includes(codeword)
      && ['done', 'completed'].includes((result.worker || result).status), 'hidden worker report');
    throw new Error(JSON.stringify({ runs, report, reason: 'Dreamer has no provider/workspace override; shared DEV memory must not be consolidated by bench. Isolated dreamer adapter required.' }));
  });
  await row('12', 'native_question_round_trip', async () => {
    assert(descriptor.supports_prompt_answers, { reason: 'provider does not advertise prompt answers' });
    const questionTab = await spawnTab('Question');
    await cli('send', '--tab', questionTab, '--from-file', padFile(prompts.question), '--require-agent');
    const pending = await poll(() => inspect(questionTab), tab => tab.agent_runtime_questions?.length > 0, 'native pending question');
    const prompt = pending.agent_runtime_questions[0];
    const before = new Set((await history(questionTab)).map(message => message.id));
    const response = await cli('tab', 'respond', '1', '--tab', questionTab);
    const messages = await poll(() => history(questionTab), messages => messages.some(message =>
      message.type === 'assistant' && !before.has(message.id) && /Amber/.test(message.content)), 'question final answer');
    return { prompt, response, answer: messages.filter(message => message.type === 'assistant' && !before.has(message.id)) };
  });
} catch (error) {
  console.log(`FAIL setup: ${error.message}`);
  for (let id = 1; id <= 12; id++) if (!rows.some(row => row.id === String(id))) {
    rows.push({ id: String(id), name: 'unreached', status: 'FAIL', evidence: error.message });
  }
} finally {
  await row('cleanup', 'owned_resources_removed', async () => {
    const failures = [];
    async function cleanupCall(action) {
      try { await action(); } catch (error) { failures.push(error.message); }
    }
    for (const id of owned.workers) {
      await cleanupCall(async () => {
        const current = await cli('mem', 'worker', 'get', id);
        if (!['done', 'completed'].includes((current.worker || current).status)) await cli('mem', 'worker', 'kill', id);
      });
    }
    for (const id of owned.crons) {
      await cleanupCall(async () => {
        const jobs = await cli('cron', 'list');
        const list = Array.isArray(jobs) ? jobs : jobs.jobs || jobs.crons || [];
        if (list.some(job => job.id === id)) {
          await cli('cron', 'cancel', id);
          await cli('cron', 'delete', id);
        }
      });
    }
    if (owned.plan) await cleanupCall(() => cli('--as-tab', owned.owner, 'plan', 'delete', owned.plan, '--yes'));
    if (owned.workspace) await cleanupCall(() => cli('workspace', 'close', owned.workspace, '--force'));
    assert(failures.length === 0, { cleanupFailures: failures });
    const workspaces = await get('/api/workspaces');
    assert(!workspaces.some(ws => ws.id === owned.workspace), { remaining: owned.workspace });
    const tabs = await get('/api/tabs');
    assert(!tabs.some(tab => tab.workspace_id === owned.workspace), { reason: 'owned tabs remain' });
    return { workspace: owned.workspace, deleted: true };
  });
  writeFileSync(join(out, 'results.json'), JSON.stringify(rows, null, 2));
  writeFileSync(join(out, 'results.md'), '| Row | Result | Scenario |\n|---|---|---|\n' + rows.map(row => `| ${row.id} | ${row.status} | ${row.name} |`).join('\n') + '\n');
  console.log(`RESULTS ${out}`);
  process.exitCode = rows.some(row => row.status === 'FAIL') ? 1 : 0;
}
