import { createRequire } from 'node:module';
import { mkdirSync, writeFileSync, readFileSync, statSync, watch, appendFileSync } from 'node:fs';
import { resolve, join, dirname } from 'node:path';
import { spawn } from 'node:child_process';
import { EventEmitter, once } from 'node:events';
import { setInterval as sampleEvery } from 'node:timers/promises';
import { client, Events, root } from '../core-journeys/lib/env.mjs';
import { listenerIdentity } from '../core-journeys/lib/listener.mjs';
import { focusedPaneScript } from '../core-journeys/lib/cdp.mjs';
import { retainedSendReceipt } from '../restart-endurance/send-receipt.mjs';
import { chatMessages, replyVerdict, newAssistantText } from '../core-journeys/lib/oracle.mjs';
import { inventory, matrix, lockSites, render, evaluateSla, replyWindow, remainingDeadline, probeMarker } from './model.mjs';
import { healthPreflight, assertDevHealth } from './preflight.mjs';

const args = process.argv.slice(2);
const option = (flag, fallback) => args.includes(flag) ? args[args.indexOf(flag) + 1] : fallback;
const out = resolve(option('--out', join(root, 'target-check/evals/startup-matrix', new Date().toISOString().replaceAll(/[:.]/g, '-'))));
const expected = option('--expect-build', '3b7bf7ea2');
const deadlineMs = 120_000;
const resume = option('--resume', null);
const attach = option('--attach', null);
const preservedInputIds = new Set(option('--preserve-input', '').split(',').filter(Boolean));
const priorDir = attach ?? resume;
const previous = priorDir ? JSON.parse(readFileSync(join(resolve(priorDir), 'results.json'))) : null;
if (!attach && previous?.tabs.some(t => t.send_at != null)) throw new Error('Resume refused: the prior run already attempted prompts');
if (Number(process.env.CODETERM_API_PORT) !== 7686 || !process.env.CODETERM_DEV_DATA_DIR)
  throw new Error('Use run.sh: requires dev_run connection pinned to 7686');
const dev = { url: 'http://127.0.0.1:7686', port: 7686, token: process.env.CODETERM_API_TOKEN };
const { api } = client(dev);
const logPath = join(process.env.CODETERM_DEV_DATA_DIR, 'logs/codeterm.log');
mkdirSync(join(out, 'screenshots'), { recursive: true });
const result = { schema: 1, phase: 'preflight', build: expected, started_at: Date.now(), tabs: [], coverage: [], health: [], preflights: [], failures: [] };
const saveJson = (name, value) => writeFileSync(join(out, name), JSON.stringify(value, null, 2) + '\n');
const save = () => { saveJson('results.json', result); writeFileSync(join(out, 'results.md'), render(result)); };
const bus = new EventEmitter(); bus.setMaxListeners(100);
const captured = [];
let browser, page, events;
let logOffset = statSync(logPath).size;
const runLog = [];
writeFileSync(join(out, 'dev.log'), '');
const logWatcher = watch(dirname(logPath), (_event, file) => {
  if (file && String(file) !== 'codeterm.log') return;
  try {
    const bytes = readFileSync(logPath);
    if (bytes.length < logOffset) logOffset = 0;
    const chunk = bytes.subarray(logOffset).toString(); logOffset = bytes.length;
    appendFileSync(join(out, 'dev.log'), chunk); runLog.push(chunk);
    bus.emit('log');
  } catch (error) { result.log_error = error.message; }
});
const sampling = new AbortController();
let sampler = Promise.resolve();
function startSampling() {
  sampler = (async () => {
  // Cadence samples latency only; it never establishes readiness or advances a test.
  for await (const _ of sampleEvery(3000, null, { signal: sampling.signal })) {
    const at = Date.now();
    try { await api('/api/health', { signal: AbortSignal.timeout(5000) }); result.health.push({ at, ms: Date.now() - at, ok: true }); }
    catch (error) { result.health.push({ at, ms: Date.now() - at, ok: false, error: error.message }); }
  }
  })().catch(error => { if (error.name !== 'AbortError') result.health_error = error.message; });
}

class ReplayEvents extends Events {
  async open(boot) {
    const response = await fetch(`${dev.url}/api/events/stream?since=${boot}:0`, {
      headers: { Authorization: `Bearer ${dev.token}` }, signal: this.controller.signal,
    });
    if (!response.ok) throw new Error(`SSE ${response.status}`);
    this.reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    this.consume().catch(error => { this.fail(error); bus.emit('event'); });
    return this;
  }
  push(event) {
    const record = { received_at: Date.now(), ...event };
    captured.push(record); appendFileSync(join(out, 'events.jsonl'), JSON.stringify(record) + '\n');
    bus.emit('event', record);
  }
}
async function waitFor(read, judge, signal, name, topic = 'event') {
  for (;;) {
    signal.throwIfAborted();
    const iteration = new AbortController();
    const next = once(bus, topic, { signal: AbortSignal.any([signal, iteration.signal]) });
    next.catch(() => {});
    // Observe before reading, so a state-changing event during the HTTP request is retained.
    try {
      const value = await read();
      if (judge(value)) return value;
      await next;
    } finally { iteration.abort(); }
    if (events?.failure) throw events.failure;
  }
}
const currentState = signal => api('/api/mux/state', { signal });
const tabRecord = async (id, signal) => (await currentState(signal)).tabs[id];
async function screenshot(label) {
  const file = `screenshots/${label}.png`;
  try { await page.screenshot({ path: join(out, file), timeout: 10_000 }); return file; }
  catch (error) { return `unavailable: ${error.message}`; }
}
function cli(args, stdin) {
  return new Promise((accept, reject) => {
    const child = spawn('bash', [join(root, 'evals/startup-matrix/run.sh'), '--dc', ...args], {
      cwd: root, stdio: ['pipe', 'pipe', 'pipe'], signal: AbortSignal.timeout(deadlineMs),
    });
    let stdout = '', stderr = '';
    child.stdout.on('data', b => { stdout += b; }); child.stderr.on('data', b => { stderr += b; });
    child.on('error', reject); child.on('close', code => accept({ code, stdout, stderr })); child.stdin.end(stdin);
  });
}
async function connect(health) {
  const require = createRequire(join(resolve(option('--dependencies', root)), 'package.json'));
  const { chromium } = require('@playwright/test');
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${health.instance.cdp_port}`, { timeout: deadlineMs });
  page = browser.contexts().flatMap(c => c.pages()).find(p => p.url().startsWith(`http://127.0.0.1:${health.instance.vite_port}`));
  if (!page) throw new Error('No real DEV WebView2 target at its Vite origin');
  page.setDefaultTimeout(15_000);
  saveJson('window.json', { url: page.url(), cdp_port: health.instance.cdp_port });
}
async function select(t) {
  await selectWorkspace(t);
  const group = tabGroup(t);
  const button = tabButton(t);
  if (!await button.count()) throw new Error(`No accessible activation button for ${t.title}`);
  const at = Date.now();
  if (await button.isEnabled()) await button.click();
  else if (!await page.getByRole('region', { name: `Tab chat: ${t.title}`, exact: true }).isVisible())
    throw new Error('Tab activation is disabled and its own chat is not visible');
  await page.evaluate(focusedPaneScript(t.id, 15_000));
  return { at, render_ms: Date.now() - at, sidebar: await (await group.count() ? group.first() : button).ariaSnapshot() };
}
function tabGroup(t) {
  const group = page.getByRole('group', { name: t.role_profile === 'general_agent' ? 'General Agent' : t.title, exact: true });
  return t.role_profile === 'general_agent' ? group.filter({ hasText: t.title }) : group;
}
function tabButton(t) {
  return t.role_profile === 'general_agent'
    ? page.getByRole('button', { name: 'General Agent', exact: true }).filter({ hasText: t.title }).last()
    : tabGroup(t).getByRole('button', { name: new RegExp('^Open tab ') }).first();
}
async function selectWorkspace(t) {
  if (t.workspace !== 'General')
    await page.getByRole('listitem').getByRole('button', { name: `Select workspace ${t.workspace}`, exact: true }).click();
}
async function marker(t) {
  await selectWorkspace(t);
  const groups = tabGroup(t);
  if (await groups.count()) return groups.first().ariaSnapshot();
  const button = tabButton(t);
  return await button.count() ? button.ariaSnapshot() : 'No accessible tab activation button';
}
function stageFor(id, since) {
  return captured.find(e => e.received_at >= since && e.event_type === 'tab-boot-stage' &&
    e.data?.tab_id === id && e.data.stage === 'agent_ready');
}
async function ready(t, since, signal) {
  await waitFor(async () => stageFor(t.id, since), Boolean, signal, 'agent_ready');
  const event = stageFor(t.id, since);
  return { event, ms: event.received_at - since };
}
async function reply(t, signal, send, baseline) {
  const before = baseline ?? await api(`/api/tabs/${t.id}/chat`, { signal });
  const baselineIds = new Set(chatMessages(before).map(m => m.id));
  const record = t.boot ?? await tabRecord(t.id, signal);
  const at = Date.now();
  t.send_at = at; t.send = await send(); save();
  const outcome = await waitFor(async () => {
    const [chat, record] = await Promise.all([api(`/api/tabs/${t.id}/chat`, { signal }), tabRecord(t.id, signal)]);
    return { chat, record };
  }, value => {
    const messages = chatMessages(value.chat);
    const window = replyWindow(messages, t.prompt);
    if (!window) return false;
    t.user_turn_evidence = { id: window.user.id, session: value.record?.agent_session_id };
    const following = window.following;
    if (t.first_reply_ms == null && newAssistantText(following, baselineIds).length)
      t.first_reply_ms = Date.now() - at;
    const verdict = replyVerdict({ messages: following, baselineIds, marker: t.marker,
      record: value.record, boundSession: record.agent_session_id });
    if (verdict.fail) throw new Error(verdict.fail);
    return Boolean(verdict.done);
  }, signal, 'PONG transcript');
  const file = `transcript-${t.id}.json`; saveJson(file, { prompt: t.prompt, marker: t.marker, baseline_ids: [...baselineIds], session_before: record.agent_session_id, ...outcome });
  t.completed_ms = Date.now() - at; t.reply_ms = t.first_reply_ms; t.reply_ok = true; t.reply_evidence = file;
}
async function main() {
  const preflight = await healthPreflight({ read: signal => api('/api/health', { signal }) });
  result.preflights.push(preflight);
  saveJson('preflight-entry.json', preflight); save();
  if (!preflight.responded) throw new Error(`api_unresponsive: no health response within 30000 ms (${preflight.ms} ms): ${preflight.error}`);
  assertDevHealth(preflight.health);
  const before = attach ? preflight.health : previous?.before_health ?? preflight.health;
  if (!resume && !before.build_sha?.startsWith(expected) && !before.build?.startsWith(expected)) throw new Error(`Wrong build ${before.build}`);
  result.phase = 'matrix';
  startSampling();
  result.build = before.build_sha ?? before.build;
  result.before_health = before;
  result.before_listener = attach ? previous.restart.after : previous?.before_listener ?? listenerIdentity(7686);
  const state = resume ? JSON.parse(readFileSync(join(resolve(resume), 'before-state.json'))) : await currentState();
  saveJson('before-state.json', state);
  const rows = inventory(state, result.started_at);
  result.inventory = rows;
  result.tabs = matrix(rows).map(t => ({ ...t, role_profile: state.tabs[t.id].role_profile, notes: [], reply_ok: false, observed: 'not measured', ready_ms: null, reply_ms: null }));
  if (!priorDir) {
    await connect(before);
    for (const t of result.tabs) t.before_sidebar = await marker(t).catch(e => `unavailable: ${e.message}`);
  } else {
    result.resumed_from = resolve(priorDir);
    for (const t of result.tabs) t.before_sidebar = previous.tabs.find(p => p.id === t.id)?.before_sidebar;
  }
  save(); console.log(`ARMED ${out}`);
  // The Orchestrator owns restart. Log writes are observation cues, never proof of a restart.
  const newHealth = attach ? before : await waitFor(() => api('/api/health', { signal: AbortSignal.timeout(5000) }).catch(() => null),
    h => h?.boot_id && h.boot_id !== before.boot_id, AbortSignal.timeout(deadlineMs), 'new boot', 'log');
  result.restart = { before: result.before_listener, after: listenerIdentity(7686), health: newHealth };
  assertDevHealth(newHealth);
  result.restart.verified = result.restart.before.pid !== result.restart.after.pid &&
    result.restart.before.started_us !== result.restart.after.started_us;
  if (!result.restart.verified) throw new Error('Restart identity proof failed');
  if (!newHealth.build_sha?.startsWith(expected) && !newHealth.build?.startsWith(expected)) throw new Error(`Wrong measured build ${newHealth.build}`);
  result.build = newHealth.build_sha ?? newHealth.build;
  const bootAt = result.restart.after.started_us / 1000;
  result.restart.started_at = bootAt;
  const bytes = readFileSync(logPath);
  const bootLog = bytes.toString().split('\n').filter(line =>
    Date.parse(line.match(/^\[([^\s\]]+)/)?.[1] ?? '') >= Math.floor(bootAt / 1000) * 1000).join('\n') + '\n';
  runLog.splice(0, runLog.length, bootLog); logOffset = bytes.length;
  writeFileSync(join(out, 'dev.log'), bootLog);
  events = await new ReplayEvents(dev, new AbortController().signal).open(newHealth.boot_id);
  await browser?.close(); await connect(newHealth);
  writeFileSync(join(out, 'accessibility.txt'), await page.locator('body').ariaSnapshot());
  const bootState = await currentState(); saveJson('boot-state.json', bootState);
  result.boot_snapshot_at = Date.now();
  for (const t of result.tabs) {
    const r = bootState.tabs[t.id]; t.boot = r; t.observed = r?.frozen ? 'frozen' : r?.boot_stage ?? 'absent';
    t.boot_sidebar = await marker(t).catch(e => `unavailable: ${e.message}`);
    if (r?.frozen && !/sleep|frozen/i.test(t.boot_sidebar)) t.notes.push('Sleeping sidebar marker absent');
  }
  save();
  const awake = result.tabs.filter(t => t.policy === 'awake');
  const bootPending = awake.map(async t => {
    try { const r = await ready(t, bootAt, AbortSignal.timeout(remainingDeadline(Date.now(), bootAt, deadlineMs))); t.ready_ms = r.ms; t.ready_event = r.event; }
    catch (error) { t.notes.push(`boot readiness failed: ${error.message}`); }
  });
  const frozen = result.tabs.filter(t => t.boot?.frozen);
  const attempted = new Map(attach ? previous.tabs.filter(t => t.send_at != null).map(t => [t.id, t]) : []);
  const wakeEligible = frozen.filter(t => !preservedInputIds.has(t.id));
  const agentWake = [...wakeEligible].sort((a, b) => a.title.localeCompare(b.title)).find(t => t.backing !== 'chat_backend') ?? wakeEligible[0];
  const pending = [...bootPending];
  const ordered = [...result.tabs].sort((a, b) => Number(b.backing === 'chat_backend') - Number(a.backing === 'chat_backend') || a.title.localeCompare(b.title));
  for (const t of ordered) {
    let started = Date.now();
    t.marker = probeMarker(result.started_at, t.id);
    t.prompt = `Reply with exactly ${t.marker} and nothing else. Do not run tools.`;
    console.log(`PROBE ${t.title} ${t.id} ${t.observed}`);
    t.wake_method = t === agentWake ? 'agent-send' : t.boot?.frozen ? 'sidebar-open' : 'already-awake';
    try {
      if (preservedInputIds.has(t.id)) {
        t.notes.push('Existing input preserved by operator; no new payload or replay');
        t.input_preserved = true;
        t.click = await select(t);
        t.screenshot = await screenshot(t.id);
        continue;
      }
      if (attempted.has(t.id)) {
        const prior = attempted.get(t.id);
        t.prior_probe = { directory: resolve(attach), prompt: prior.prompt, marker: prior.marker,
          send: prior.send, reply_ok: prior.reply_ok === true };
      }
      if (t === agentWake) {
        const signal = AbortSignal.timeout(deadlineMs);
        const task = reply(t, signal, async () => {
          const receipt = await cli(['send', '--tab', t.id, '--require-agent', '--stdin', '--json'], t.prompt);
          receipt.retained = retainedSendReceipt(receipt.code, receipt.stdout);
          if (receipt.code && !receipt.retained) t.notes.push(`dc send exit ${receipt.code}`);
          return receipt;
        });
        pending.push(task.then(async () => {
          const r = await ready(t, started, signal); t.wake_ms = r.ms; t.wake_event = r.event;
        }).catch(async error => { t.notes.push(`agent wake/reply failed: ${error.message}`); t.screenshot = await screenshot(t.id); }).finally(save));
        continue;
      }
      const baseline = await api(`/api/tabs/${t.id}/chat`).catch(error => {
        t.baseline_error = error.message; return [];
      });
      started = Date.now();
      t.click = await select(t);
      const signal = AbortSignal.timeout(deadlineMs);
      const composer = page.getByRole('region', { name: `Tab chat: ${t.title}`, exact: true })
        .getByRole('textbox', { name: 'Message composer', exact: true }).filter({ visible: true });
      if (t.backing === 'chat_backend' && !await composer.count()) {
        t.notes.push('No visible chat composer after tab click');
        t.screenshot = await screenshot(t.id);
        pending.push(ready(t, started, signal).then(r => { t.wake_ms = r.ms; t.wake_event = r.event; })
          .catch(e => t.notes.push(`click wake failed: ${e.message}`)).finally(save));
      } else {
        // Fill and submit before selecting the next tab; transcript observation can then run concurrently.
        let sent;
        const delivered = new Promise((accept, reject) => { sent = { accept, reject }; });
        const task = reply(t, signal, async () => {
          try {
            if (t.backing === 'chat_backend') {
              const existing = await composer.inputValue();
              if (existing) throw new Error('Owner has a composer draft; preserved');
              await composer.fill(t.prompt);
              t.accepting_ms = Date.now() - (t.click.at + t.click.render_ms);
              t.boot_accepting_upper_bound_ms = Date.now() - bootAt;
              await composer.press('Enter');
              sent.accept(); return { method: 'composer-enter' };
            }
            const receipt = await cli(['send', '--tab', t.id, '--require-agent', '--stdin', '--json'], t.prompt);
            receipt.retained = retainedSendReceipt(receipt.code, receipt.stdout);
            if (receipt.code && !receipt.retained) t.notes.push(`Send refused or unconfirmed: exit ${receipt.code}`);
            sent.accept(); return receipt;
          } catch (error) { sent.reject(error); throw error; }
        }, baseline);
        pending.push(task.then(async () => {
          if (t.boot?.frozen) { const r = await ready(t, started, signal); t.wake_ms = r.ms; t.wake_event = r.event; }
        }).catch(async e => { t.notes.push(`reply/wake failed: ${e.message}`); t.screenshot ??= await screenshot(t.id); }).finally(save));
        await Promise.race([delivered, task]);
      }
    } catch (error) { t.notes.push(`UI action failed: ${error.message}`); t.screenshot = await screenshot(t.id); save(); }
  }
  await Promise.all(pending);
  if (captured.some(e => e.event_type === 'overflow')) result.failures.push('SSE replay/stream gap; see overflow frames in events.jsonl');
  const finalState = await currentState().catch(error => {
    result.failures.push(`Final state unavailable: ${error.message}`); return { tabs: {} };
  });
  result.final_state = 'final-state.json'; saveJson(result.final_state, finalState);
  for (const t of result.tabs) {
    const final = finalState.tabs[t.id];
    t.final = final;
    t.observed += ` → ${final?.frozen ? 'still frozen' : final?.boot_stage ?? 'absent'}`;
    if (final?.boot_failure_reason) t.notes.push(final.boot_failure_reason);
    if (t.policy === 'awake' && t.ready_ms == null) t.notes.push('No boot agent_ready event within 120 s');
    t.started = t.ready_ms != null || t.wake_event != null;
    t.sla = evaluateSla(t);
    if (!t.sla.pass) t.notes.push(`SLA FAILURE: ${t.sla.surface} ${t.sla.measured_ms ?? 'unproven'} ms, limit ${t.sla.limit_ms} ms`);
    if ((!t.reply_ok || t.notes.length) && !t.screenshot) {
      try { await select(t); } catch (error) { t.notes.push(`failure capture selection: ${error.message}`); }
      t.screenshot = await screenshot(t.id);
    }
  }
  result.coverage.push(`General=${result.tabs.filter(t => t.workspace === 'General').length}`,
    `recent=${result.tabs.filter(t => t.last_active_at_ms != null && t.policy === 'awake').length}`,
    `frozen-open=${frozen.length - (agentWake ? 1 : 0)}`, `frozen-agent=${agentWake ? 1 : 0}`,
    `Endure=${result.tabs.filter(t => t.workspace === 'Endure').length}`);
  for (const title of ['codexF', 'claudeF', 'grokF', 'aiderF', 'opencodeF', 'codexU', 'claudeU', 'grokU', 'aiderU', 'opencodeU'])
    if (!result.tabs.some(t => t.workspace === 'Endure' && t.title === title)) result.failures.push(`Missing Endure ${title}`);
  if (!agentWake) result.failures.push('No frozen agent-send fixture');
  if (!result.tabs.some(t => t.last_active_at_ms != null && t.policy === 'awake')) result.failures.push('No recent fixture');
}
try { await main(); }
catch (error) { result.failures.push(error.stack); console.error(error.stack); }
finally {
  sampling.abort(); await sampler; events?.close(); logWatcher.close();
  result.finished_at = Date.now();
  const fixedBoot = result.restart?.started_at;
  const measuredLines = runLog.join('').split('\n').filter(line => !fixedBoot ||
    Date.parse(line.match(/^\[([^\s\]]+)/)?.[1] ?? '') >= Math.floor(fixedBoot / 1000) * 1000);
  result.lock_sites = lockSites(measuredLines);
  writeFileSync(join(out, 'lock-holds.md'), '# Run-window lock holds and slow IPC\n\n| site | count | total ms | max ms |\n|---|---:|---:|---:|\n' +
    result.lock_sites.map(s => `| ${s.site} | ${s.count} | ${s.total_ms} | ${s.max_ms} |`).join('\n') +
    '\n\n```text\n' + result.lock_sites.flatMap(s => s.lines).join('\n') + '\n```\n');
  save(); await browser?.close().catch(() => {});
  console.log(`RESULTS ${out}`);
  process.exitCode = result.failures.length || result.preflights.some(p => p.verdict === 'fail') || result.tabs.some(t => !t.reply_ok || t.notes.length) ? 3 : 0;
}
