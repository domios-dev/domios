// WHAT: Replay verifier reads the restored desk over one composed DEV connection.
// WHY: A shell plus lib resolution per CLI read (about 14 s each under load) elapsed the 90 s deadline on every tab.
// ORACLE: Live payload shapes, request paths, deadline mapping and isolation refusals with an injected fetch.
import assert from 'node:assert/strict';
import test from 'node:test';
import {fileURLToPath} from 'node:url';
import {worstCases} from './fixtures.mjs';
import {historyMatches, requireBackings, synthesizeHistory, verdict} from './model.mjs';
import {devApi, deadlineError, isolatedConnection} from './transport.mjs';
import {observe, touchesTab} from '../core-journeys/lib/wait.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
const dev = {url: 'http://127.0.0.1:17698', token: 'fixture-token', instance: 'upgrade-restore-fixture'};

const PRODUCT_PAGE_ITEMS = 16;

function restoredDaemon(document, {pagesLoaded = Number.POSITIVE_INFINITY} = {}) {
  const requests = [];
  const pages = new Map();
  const shapeOf = tab => document.histories.find(history => history.tab_id === tab).shape;
  const loadedRows = tab => Math.min(shapeOf(tab).length, (pages.get(tab) ?? pagesLoaded) * PRODUCT_PAGE_ITEMS);
  const runtime = tab => {
    const total = shapeOf(tab).length;
    const loaded = loadedRows(tab);
    return {state: 'loaded', epoch: 1, history: loaded >= total ? 'complete' : 'loading',
      historyPages: Math.ceil(loaded / PRODUCT_PAGE_ITEMS) + 1};
  };
  const pageIn = tab => pages.set(tab, Math.ceil(loadedRows(tab) / PRODUCT_PAGE_ITEMS) + 1);
  const tabs = new Map(requireBackings(document).map(tab => [tab.tab_id, {...tab,
    task_binding: tab.agent_identity?.task_binding && {...tab.agent_identity.task_binding,
      attempt_id: `attempt_${tab.tab_id}`},
    session_binding_nonce: `nonce-${tab.tab_id}`, reported_session_binding_nonce: `nonce-${tab.tab_id}`}]));
  const product = row => ({id: row.id, type: row.type, content: row.content + (row.attachments ?? [])
    .map(image => `\n[image omitted: ${Buffer.byteLength(`data:image/png;base64,${image.data}`)} bytes]`).join(''),
  ...(row.attachments ? {attachments: row.attachments.map(({id}) => ({id, mimeType: 'image/png'}))} : {})});
  async function fetch(url, options) {
    assert.equal(options.headers.Authorization, `Bearer ${dev.token}`);
    const {pathname, searchParams} = new URL(url);
    requests.push(`${options.method} ${pathname}`);
    const tab = decodeURIComponent(pathname.split('/')[3] ?? '');
    let body;
    if (pathname === '/api/tabs/inspect') {
      const id = searchParams.get('tab_ids');
      body = [{...tabs.get(id), agent_runtime_state: runtime(id)}];
    } else if (pathname.endsWith('/focus')) body = {ok: true};
    else if (pathname.endsWith('/chat') && searchParams.get('since') === '0') {
      const messages = synthesizeHistory(shapeOf(tab)).slice(0, loadedRows(tab)).map(product);
      body = {count: messages.length, messages, pending: false};
    } else return new Response('missing', {status: 404});
    return new Response(JSON.stringify(body), {status: 200});
  }
  return {fetch, requests, pageIn};
}

const firstMissing = (expected, messages) =>
  expected.find(row => !historyMatches(row, messages.find(message => message.id === row.id)))?.id;

test('verify reads every restored worst-case tab and its full history without a per-read shell', async () => {
  const document = worstCases(root, '/isolated-fixture-project');
  const daemon = restoredDaemon(document);
  const api = devApi(dev, 60_000, {now: () => 0, fetch: daemon.fetch});
  const results = await Promise.all(requireBackings(document).map(async tab => {
    await api.focus(tab.tab_id);
    const record = await api.inspect(tab.tab_id);
    const {messages} = await api.chat(tab.tab_id);
    const expected = synthesizeHistory(document.histories.find(history => history.tab_id === tab.tab_id).shape);
    assert.ok(expected.every(row => historyMatches(row, messages.find(message => message.id === row.id))));
    return verdict(tab, record);
  }));
  assert.deepEqual(results.map(result => result.ready), [true, true, true, true, true, true]);
  assert.equal(daemon.requests.length, 18);
  assert.ok(daemon.requests.includes('POST /api/tabs/fixture-planner/focus'));
  assert.ok(daemon.requests.includes('GET /api/tabs/fixture-long-history/chat'));
});

test('a loaded runtime still paging history is not ready, so chat is read only after the last page', async () => {
  const document = worstCases(root, '/isolated-fixture-project');
  const tab = requireBackings(document).find(row => row.tab_id === 'fixture-long-history');
  const expected = synthesizeHistory(document.histories.find(history => history.tab_id === tab.tab_id).shape);
  const daemon = restoredDaemon(document, {pagesLoaded: 1});
  const api = devApi(dev, 60_000, {now: () => 0, fetch: daemon.fetch});
  const partial = await api.inspect(tab.tab_id);
  assert.equal(partial.agent_runtime_state.state, 'loaded');
  assert.equal(firstMissing(expected, (await api.chat(tab.tab_id)).messages), 'message-16');
  assert.deepEqual(verdict(tab, partial), {tab_id: tab.tab_id, ready: false, reason: 'history loading after 2 pages'});
  let wakes = 0;
  const events = {drain() {}, async next() { wakes++; daemon.pageIn(tab.tab_id); return {tab_id: tab.tab_id}; }};
  await observe({events, filter: touchesTab(tab.tab_id), label: tab.tab_id, read: () => api.inspect(tab.tab_id),
    judge: actual => verdict(tab, actual).ready ? {done: actual} : {pending: verdict(tab, actual).reason}});
  assert.equal(wakes, 19);
  assert.equal(firstMissing(expected, (await api.chat(tab.tab_id)).messages), undefined);
});

test('failed history paging ends readiness with its page and error', () => {
  const tab = requireBackings(worstCases(root, '/isolated-fixture-project'))[2];
  const actual = {...tab, task_binding: undefined, session_binding_nonce: 'nonce', reported_session_binding_nonce: 'nonce',
    agent_runtime_state: {state: 'loaded', epoch: 1, history: 'failed', historyPages: 3, historyError: 'fixture page refused'}};
  assert.deepEqual(verdict(tab, actual),
    {tab_id: tab.tab_id, ready: false, terminal: true, reason: 'history failed after 3 pages: fixture page refused'});
});

test('an elapsed or aborted deadline reports the bounded replay reason', async () => {
  let now = 0;
  const pending = devApi(dev, 10, {now: () => now,
    fetch: async () => { throw new DOMException('The operation timed out.', 'TimeoutError'); }});
  await assert.rejects(pending.health(), error => error.killed && error.message === deadlineError().message);
  now = 10;
  let calls = 0;
  const elapsed = devApi(dev, 10, {now: () => now, fetch: async () => { calls++; }});
  await assert.rejects(elapsed.inspect('fixture-planner'), /bounded replay deadline elapsed/);
  assert.equal(calls, 0);
});

test('refusals keep their HTTP status for the readiness observer', async () => {
  const api = devApi(dev, 60_000, {now: () => 0, fetch: async () => new Response('starting', {status: 503})});
  await assert.rejects(api.inspect('fixture-planner'), error => error.status === 503 && /HTTP 503 starting/.test(error.message));
});

test('the composed connection must be a named isolated upgrade-restore instance', () => {
  assert.equal(isolatedConnection(dev), dev);
  for (const refused of [{...dev, instance: ''}, {...dev, instance: 'other'}, {...dev, token: ''},
    {...dev, url: 'http://127.0.0.1:7685'}, {...dev, url: 'http://127.0.0.1:7686'}]) {
    assert.throws(() => devApi(refused, Number.POSITIVE_INFINITY), /named upgrade-restore-\* DEV instance/);
  }
});
