import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import {
  createLookup, createSession, EPOCH_TURNS, MIN_PAGE_CHARS, MAX_PAGE_CHARS, SYSTEM_PROMPT, toolSchemas,
} from './protocol.mjs';

const scenarioNames = ['long-session', 'mem-enrichment'];
const defaultBaseUrl = 'http://127.0.0.1:1234/v1';

export async function loadScenario(name) {
  if (!scenarioNames.includes(name)) throw new Error(`Unknown scenario: ${name}`);
  return JSON.parse(await readFile(new URL(`scenarios/${name}.json`, import.meta.url), 'utf8'));
}

function mockModel(turn, throughSeq, enabled, messages) {
  const toolCalls = [];
  const { handoff, pages } = JSON.parse(messages[1].content);
  const lookupResult = messages.find(message => message.role === 'tool');
  const tool = (name, args) => toolCalls.push({
    id: `mock-${toolCalls.length}`, type: 'function', function: { name, arguments: JSON.stringify(args) },
  });
  if (turn.lookup && enabled && !lookupResult) {
    tool('mem_lookup', { query: turn.lookup });
  } else if (turn.hold) {
    tool('mark_seen', { through_seq: throughSeq });
  } else {
    for (const route of turn.routes ?? []) {
      const { target, payload, intent, attachments } = route;
      const visibleAttachments = pages.flatMap(page => page.attachments);
      const heldUnread = pages[0].meta.seen_seq > pages[0].meta.acked_seq;
      if (turn.resumeSeen && !heldUnread) continue;
      if (turn.priorRouteTab && !handoff.routes.some(route => route.args.target_tab === turn.priorRouteTab
        && matchesTerms(route.args.payload, intent))) continue;
      if (attachments?.some(id => !visibleAttachments.some(ref => ref.id === id
        && ref.seq > pages[0].meta.acked_seq))) continue;
      if (turn.lookup && (!lookupResult || !normalize(lookupResult.content).includes(normalize(turn.memoryEvidence)))) continue;
      tool('route', { target_tab: target, payload, rationale: 'Requested in the transcript' });
    }
    if (turn.patch) tool('shape_doc', { patch: turn.patch });
    if (turn.fact) tool('save_mem', { fact: turn.fact });
    tool('ack', { through_seq: throughSeq, note: turn.lookup && !enabled ? 'Reference unavailable' : 'Processed' });
  }
  const message = { role: 'assistant', content: null, tool_calls: toolCalls };
  return { message, usage: { prompt_tokens: 220, completion_tokens: 12 * toolCalls.length } };
}

export function httpModel({ baseUrl = defaultBaseUrl, model, apiKey, fetcher = fetch }) {
  const endpoint = new URL(baseUrl);
  if (!['http:', 'https:'].includes(endpoint.protocol) || endpoint.username || endpoint.password
    || endpoint.search || endpoint.hash) throw new Error('Expected an HTTP(S) API base URL without credentials, query or fragment');
  endpoint.pathname = endpoint.pathname.replace(/\/+$/, '') + '/chat/completions';
  return async (messages, tools) => {
    const response = await fetcher(endpoint, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(apiKey ? { Authorization: `Bearer ${apiKey}` } : {}),
      },
      body: JSON.stringify({ model, messages, tools, tool_choice: 'auto', temperature: 0, stream: false }),
      signal: AbortSignal.timeout(60_000),
      redirect: 'error',
    });
    if (!response.ok) throw new Error(`Chat completions HTTP ${response.status}`);
    const result = await response.json();
    const message = result.choices?.[0]?.message;
    if (!message) throw new Error('Chat completions returned no assistant message');
    return { message, usage: result.usage };
  };
}

async function runTurn(messages, tools, lookup, request) {
  const usage = { prompt_tokens: 0, completion_tokens: 0, requests_with_usage: 0, requests: 0 };
  try {
    for (let requestIndex = 0; requestIndex < 3; requestIndex++) {
      usage.requests++;
      const result = await request(messages, tools);
      if (Number.isSafeInteger(result.usage?.prompt_tokens) && Number.isSafeInteger(result.usage?.completion_tokens)
        && result.usage.prompt_tokens >= 0 && result.usage.completion_tokens >= 0) {
        usage.prompt_tokens += result.usage.prompt_tokens;
        usage.completion_tokens += result.usage.completion_tokens;
        usage.requests_with_usage++;
      }
      const calls = result.message.tool_calls ?? [];
      if (!Array.isArray(calls) || calls.length > 16) throw new Error('Invalid tool batch');
      const actions = calls.map(call => {
        if (call.type !== 'function' || typeof call.id !== 'string'
          || typeof call.function?.arguments !== 'string') throw new Error('Invalid function call');
        return { name: call.function.name, args: JSON.parse(call.function.arguments) };
      });
      if (!actions.some(action => action.name === 'mem_lookup')) return { actions, usage, error: null };
      if (actions.some(action => action.name !== 'mem_lookup')) throw new Error('Lookup and writes require separate batches');

      messages.push({ role: 'assistant', content: result.message.content ?? null, tool_calls: calls });
      for (const [index, { args }] of actions.entries()) {
        const content = lookup.read(args && Object.keys(args).length === 1 ? args.query : undefined);
        messages.push({ role: 'tool', tool_call_id: calls[index].id, content });
      }
    }
    throw new Error('No cursor action after lookup budget');
  } catch (error) {
    return { actions: [], usage, error: error.message };
  }
}

function counts() {
  return { hits: 0, total: 0 };
}

function measure(counter, passed) {
  counter.total++;
  counter.hits += Number(passed);
}

function rate({ hits, total }) {
  return total ? hits / total : null;
}

function normalize(text) {
  return text.normalize('NFKC').toLowerCase().replace(/[\u2010-\u2015]/gu, '-').replace(/\s+/gu, ' ').trim();
}

function matchesTerms(text, terms) {
  return terms.every(term => new RegExp(`(?:^|[^\\p{L}\\p{N}])(?:${term})(?=$|[^\\p{L}\\p{N}])`, 'u')
    .test(normalize(text)));
}

function routeCount(expected, actual, attachmentIds) {
  const pending = [...expected];
  for (const { args } of actual) {
    const ids = attachmentIds.filter(id => matchesTerms(args.payload, [id.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')]));
    const match = pending.findIndex(route => route.target === args.target_tab
      && matchesTerms(args.payload, route.intent)
      && JSON.stringify(ids.toSorted()) === JSON.stringify((route.attachments ?? []).toSorted()));
    if (match >= 0) pending.splice(match, 1);
  }
  return expected.length - pending.length;
}

function shapeMatches(expected, actual) {
  const patch = actual[0]?.args.patch;
  if (!expected) return actual.length === 0;
  if (actual.length !== 1 || normalize(patch.heading) !== normalize(expected.heading)
    || Boolean(patch.prose.trim()) !== Boolean(expected.prose) || patch.bullets.length !== expected.bullets.length) return false;
  const concepts = text => normalize(text).match(/[\p{L}]{5,}/gu) ?? [];
  const covers = (want, got) => {
    const terms = concepts(want);
    const words = new Set(concepts(got));
    return terms.filter(word => words.has(word)).length >= Math.ceil(terms.length * 0.6);
  };
  const unmatched = [...patch.bullets];
  for (const bullet of expected.bullets) {
    const index = unmatched.findIndex(candidate => covers(bullet, candidate));
    if (index < 0) return false;
    unmatched.splice(index, 1);
  }
  return covers(expected.prose, patch.prose);
}

function percentile(values, fraction) {
  if (!values.length) return null;
  return values.toSorted((a, b) => a - b)[Math.ceil(values.length * fraction) - 1];
}

export function score(results) {
  const ambient = counts(), holds = counts(), epochs = counts(), shapes = counts();
  const memory = counts(), restarts = counts(), accuracy = counts(), savedFacts = counts();
  let routeHits = 0, routeExpected = 0, routeActual = 0, rotations = 0, errors = 0;
  for (const result of results) {
    const { turn, effects, cursorAction, error, rotated, restart } = result;
    const routes = effects.filter(effect => effect.name === 'route');
    const patches = effects.filter(effect => effect.name === 'shape_doc');
    const facts = effects.filter(effect => effect.name === 'save_mem');
    const expectedRoutes = turn.routes ?? [];
    const matchedRoutes = routeCount(expectedRoutes, routes, result.attachmentIds);
    const exactRoutes = matchedRoutes === expectedRoutes.length && matchedRoutes === routes.length;
    const shape = shapeMatches(turn.patch, patches);
    const fact = turn.fact ? facts.length === 1 && matchesTerms(facts[0].args.fact, turn.factTerms) : facts.length === 0;
    const passed = !error && exactRoutes && shape && fact && cursorAction === (turn.hold ? 'mark_seen' : 'ack');
    measure(accuracy, passed);
    if (!turn.hold && !expectedRoutes.length && !turn.patch && !turn.fact) measure(ambient, effects.length > 0);
    if (turn.hold) measure(holds, !error && cursorAction === 'mark_seen');
    if (turn.patch) measure(shapes, shape);
    if (turn.fact) measure(savedFacts, fact);
    if (turn.epochProbe) measure(epochs, passed && result.epochTurn === 1);
    if (turn.lookup) measure(memory, passed);
    if (turn.restartAfter) measure(restarts, restart);
    routeHits += matchedRoutes;
    routeExpected += expectedRoutes.length;
    routeActual += routes.length;
    rotations += Number(rotated);
    errors += Number(Boolean(error));
  }
  const latency = turns => ({
    p50: percentile(turns.map(turn => turn.latencyMs), 0.5),
    p95: percentile(turns.map(turn => turn.latencyMs), 0.95),
  });
  const measured = results.filter(result => result.usage.requests === result.usage.requests_with_usage);
  const averageTokens = key => measured.length
    ? measured.reduce((sum, turn) => sum + turn.usage[key], 0) / measured.length : null;
  return {
    turns: results.length, errors, rotations,
    ambient: { ...ambient, falsePositiveRate: rate(ambient) },
    routing: { matched: routeHits, expected: routeExpected, actual: routeActual,
      precision: rate({ hits: routeHits, total: routeActual }), recall: rate({ hits: routeHits, total: routeExpected }) },
    holds: { ...holds, accuracy: rate(holds) }, shapes: { ...shapes, completeness: rate(shapes) },
    epochs: { ...epochs, continuity: rate(epochs) }, restarts: { ...restarts, continuity: rate(restarts) },
    memory: { ...memory, accuracy: rate(memory), latencyMs: latency(results.filter(result => result.turn.lookup)) },
    savedFacts: { ...savedFacts, accuracy: rate(savedFacts) },
    accuracy: rate(accuracy), latencyMs: latency(results),
    tokensPerTurn: { measured: measured.length, prompt: averageTokens('prompt_tokens'),
      completion: averageTokens('completion_tokens') },
  };
}

export async function evaluateScenario(scenario, { mock = false, lookupEnabled = false, maxChars, request } = {}) {
  let session = createSession(scenario.tabs);
  let activeTab = 'live-input';
  const tools = toolSchemas(scenario.tabs, lookupEnabled);
  const attachmentIds = [...new Set(scenario.turns.flatMap(turn => turn.rows)
    .flatMap(row => (row.attachments ?? []).map(ref => ref.id)))];
  const results = [];
  for (const turn of scenario.turns) {
    const start = performance.now();
    if (turn.switchTo) activeTab = turn.switchTo;
    session.append(turn.rows);
    const pages = session.pages(maxChars);
    const throughSeq = pages.at(-1).meta.through_seq;
    const { epochTurns } = session.snapshot;
    const messages = [
      { role: 'system', content: SYSTEM_PROMPT },
      { role: 'user', content: JSON.stringify({
        workspace_tabs: scenario.tabs, active_tab: activeTab, transcript_source: 'live-input',
        handoff: session.handoff(), pages,
      }) },
    ];
    const lookup = createLookup(scenario.corpus, lookupEnabled);
    const response = await runTurn(messages, tools, lookup, mock
      ? messages => mockModel(turn, throughSeq, lookupEnabled, messages) : request);
    const finish = session.finish(response.actions, throughSeq);
    let restart = null;
    if (turn.restartAfter) {
      const before = session.snapshot;
      session = createSession(scenario.tabs, JSON.parse(JSON.stringify(before)));
      const restored = JSON.stringify(session.snapshot) === JSON.stringify(before);
      if (turn.restartAfter === 'committed' && !finish.error
        && response.actions.at(-1)?.name === 'ack' && finish.effects.some(effect => effect.name === 'route')) {
        const replay = session.finish(response.actions, throughSeq);
        restart = !finish.error && restored && Boolean(replay.error) && replay.effects.length === 0
          && JSON.stringify(session.snapshot) === JSON.stringify(before);
      } else if (turn.restartAfter === 'held') {
        const after = session.pages(maxChars);
        restart = !finish.error && restored && after[0].meta.acked_seq < after[0].meta.seen_seq
          && after.some(page => page.text.split('\n[SEEN]\n')[1]?.split('\n[LIVE]\n')[0]?.length > 0);
      }
    }
    results.push({
      turn, throughSeq, attachmentIds,
      ...finish, epochTurn: epochTurns + 1, restart, usage: response.usage, lookup: lookup.usage,
      cursorAction: finish.error ? null : response.actions.at(-1).name,
      error: response.error ?? finish.error,
      latencyMs: mock ? response.usage.requests * 5 : Math.round((performance.now() - start) * 100) / 100,
    });
  }
  return { results, snapshot: session.snapshot, handoff: session.handoff() };
}

export async function benchmark({ model, mock = false, memLookup = 'off', scenario = 'all',
  maxChars = 4096, request } = {}) {
  const names = scenario === 'all' ? scenarioNames : [scenario];
  const scenarios = await Promise.all(names.map(loadScenario));
  const modes = memLookup === 'ab' ? ['off', 'on'] : [memLookup];
  const configurations = [];
  for (const mode of modes) {
    const reports = [];
    const combined = [];
    for (const fixture of scenarios) {
      const { results } = await evaluateScenario(fixture, { mock, lookupEnabled: mode === 'on', maxChars, request });
      combined.push(...results);
      reports.push({ name: fixture.name, durationSeconds: fixture.turns.at(-1).at_seconds, ...score(results),
        turnsDetail: results.map(result => ({
          throughSeq: result.throughSeq, atSeconds: result.turn.at_seconds, epoch: result.epoch,
          epochTurn: result.epochTurn, cursorAction: result.cursorAction, latencyMs: result.latencyMs,
          usage: result.usage, lookup: result.lookup, error: result.error,
          effects: result.effects, expectedTargets: (result.turn.routes ?? []).map(route => route.target),
        })) });
    }
    configurations.push({ memLookup: mode, ...score(combined), scenarios: reports });
  }
  const [off, on] = configurations;
  const difference = (left, right) => left === null || right === null ? null : right - left;
  const delta = on ? {
    accuracy: difference(off.accuracy, on.accuracy),
    memoryAccuracy: difference(off.memory.accuracy, on.memory.accuracy),
    memoryLatencyMs: {
      p50: difference(off.memory.latencyMs.p50, on.memory.latencyMs.p50),
      p95: difference(off.memory.latencyMs.p95, on.memory.latencyMs.p95),
    },
  } : null;
  return { model: model ?? 'mock-llm', mock, maxChars, epochTurns: EPOCH_TURNS, configurations, delta };
}

export function markdown(report) {
  const pct = value => value === null ? '—' : `${(value * 100).toFixed(1)}%`;
  const number = value => value === null ? '—' : value.toFixed(1);
  const lines = [
    '# Live Transcribe scorecard',
    '',
    `Model ${JSON.stringify(report.model)} · ${report.mock ? 'mock cost estimates' : 'measured HTTP latency'} · ${report.maxChars} characters per page`,
    '',
    '| Lookup | Turns | Errors | Ambient FP | Route P / R | Hold | Epoch continuity | Restart | Shape | Memory probes | p50 / p95 ms | Prompt / completion tokens per turn |',
    '| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- | --- |',
  ];
  for (const config of report.configurations) {
    const { routing, epochs, memory, latencyMs, tokensPerTurn } = config;
    lines.push(`| ${config.memLookup} | ${config.turns} | ${config.errors} | ${pct(config.ambient.falsePositiveRate)} | ${pct(routing.precision)} / ${pct(routing.recall)} | ${pct(config.holds.accuracy)} | ${pct(epochs.continuity)} (${config.rotations} rotations) | ${pct(config.restarts.continuity)} | ${pct(config.shapes.completeness)} | ${pct(memory.accuracy)} | ${number(latencyMs.p50)} / ${number(latencyMs.p95)} | ${number(tokensPerTurn.prompt)} / ${number(tokensPerTurn.completion)} |`);
  }
  if (report.delta) {
    const pp = value => value === null ? '—' : `${(value * 100).toFixed(1)} pp`;
    lines.push('', `Lookup on minus off · overall accuracy ${pp(report.delta.accuracy)} · memory probe accuracy ${pp(report.delta.memoryAccuracy)} · memory probe p50 / p95 ${number(report.delta.memoryLatencyMs.p50)} / ${number(report.delta.memoryLatencyMs.p95)} ms`);
  }
  return lines.join('\n');
}

async function main() {
  const { values } = parseArgs({ options: {
    'base-url': { type: 'string', default: defaultBaseUrl }, model: { type: 'string' },
    'api-key-env': { type: 'string' }, 'mock-llm': { type: 'boolean' },
    'mem-lookup': { type: 'string', default: 'off' }, scenario: { type: 'string', default: 'all' },
    'max-chars': { type: 'string', default: '4096' }, format: { type: 'string', default: 'both' },
    help: { type: 'boolean' },
  } });
  if (values.help) {
    process.stdout.write(`Usage: node evals/live-transcribe/runner.mjs --model MODEL [--base-url URL] [--api-key-env NAME]
  [--mock-llm] [--mem-lookup on|off|ab] [--scenario ${scenarioNames.join('|')}|all]
  [--max-chars N] [--format both|markdown|json]\n`);
    return;
  }
  const mock = Boolean(values['mock-llm']);
  const memLookup = values['mem-lookup'];
  const maxChars = Number(values['max-chars']);
  const format = values.format;
  if (!mock && !values.model?.trim()) throw new Error('Use --model MODEL or --mock-llm');
  if (!['off', 'on', 'ab'].includes(memLookup)) throw new Error('Expected --mem-lookup on|off|ab');
  if (!['both', 'markdown', 'json'].includes(format)) throw new Error('Expected --format both|markdown|json');
  if (!Number.isInteger(maxChars) || maxChars < MIN_PAGE_CHARS || maxChars > MAX_PAGE_CHARS) {
    throw new Error(`Expected --max-chars in ${MIN_PAGE_CHARS}–${MAX_PAGE_CHARS}`);
  }
  const envName = values['api-key-env'];
  const apiKey = !mock && envName ? process.env[envName] : undefined;
  if (!mock && envName && !apiKey) throw new Error(`Missing API key environment variable ${envName}`);
  const request = mock ? undefined : httpModel({ baseUrl: values['base-url'], model: values.model, apiKey });
  const report = await benchmark({ model: values.model, mock, memLookup, scenario: values.scenario, maxChars, request });
  if (format !== 'json') process.stdout.write(`${markdown(report)}\n`);
  if (format === 'both') process.stdout.write('\n```json\n');
  if (format !== 'markdown') process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  if (format === 'both') process.stdout.write('```\n');
  if (report.configurations.every(config => config.errors === config.turns)) process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(error => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  });
}
