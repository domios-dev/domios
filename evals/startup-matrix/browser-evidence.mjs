import { assetIdentity, diagnosticVerdict, safeUrl, sha256 } from './evidence-model.mjs';
import { snapshotOutputEvidence, WireCapture } from './output-evidence.mjs';
import { artifacts, options, devEvidence, assertSameListener, Cdp } from './evidence-runtime.mjs';
import { captureBounded } from './phase-collector.mjs';

const args = options();
if (!args['--cdp'] || !args['--target'] || !args['--tab'] || Boolean(args['--dry-run']) === Boolean(args['--reload']))
  throw Error('--cdp URL --target ID --tab ID and exactly one of --dry-run / --reload required');
const { save, bytes } = artifacts(args['--out']);
const control = new AbortController();
const signal = AbortSignal.any([control.signal, AbortSignal.timeout(120000)]);
const result = { schema: 1, scenario: 'browser-evidence', status: 'collecting', assets: [], diagnostics: [], console: [], failures: [],
  gaps: ['Capture is not a terminal byte-integrity or UI readiness verdict', 'No actual mobile device or hidden-phone proof'] };
let cdp;
const wire = new WireCapture();
const jobs = new Set();
const streams = new Map();
let assetBytes = 0;
const track = promise => {
  const observed = promise.catch(error => { result.failures.push(error.message); control.abort(Error('Evidence operation failed')); });
  jobs.add(observed);
  observed.finally(() => jobs.delete(observed));
};
await captureBounded({ signal, persist: outcome => {
  for (const [index, stream] of wire.records().entries()) {
    const file = `output-${index}.sse`;
    bytes(file, stream.bytes);
    streams.get(stream.id).artifact = { file, bytes: stream.bytes.length, sha256: sha256(stream.bytes) };
  }
  result.streams = [...streams.values()];
  save('browser.json', { ...result, ...outcome });
  if (outcome.status !== 'completed') process.exitCode = 1;
}, close: () => { cdp?.close(); control.abort(); }, collect: async () => {
  const dev = await devEvidence(args['--expect-build'], signal);
  result.binarySha = dev.health.build_sha;
  result.listener = dev.listener;
  const state = await dev.api('/api/mux/state', { signal });
  if (!state.tabs[args['--tab']]) throw Error('Selected DEV tab absent');
  cdp = await Cdp.connect(args['--cdp'], args['--target'], signal);
  result.target = cdp.target;
  if (args['--dry-run']) {
    result.mode = 'dry-run';
    result.gaps.push('Read-only fixture validation; capture and far-end behavior unexecuted');
    return;
  }
  result.mode = 'reload-owned-page';
  const html = await fetch('http://127.0.0.1:7686/', { signal, redirect: 'error', headers: { Authorization: `Bearer ${process.env.CODETERM_API_TOKEN}` } });
  if (!html.ok) throw Error(`Served index HTTP ${html.status}`);
  const htmlChunks = []; let htmlBytes = 0;
  for await (const chunk of html.body) {
    htmlBytes += chunk.length;
    if (htmlBytes > 4 * 1024 * 1024) throw Error('Served index exceeded byte bound');
    htmlChunks.push(chunk);
  }
  const index = Buffer.concat(htmlChunks);
  result.index = assetIdentity('http://127.0.0.1:7686/', index, 'HTTP served index at capture');
  bytes(`served-index-${result.index.sha256}.html`, index);
  const recordAsset = async (event, origin) => {
    const source = await cdp.send('Debugger.getScriptSource', { scriptId: event.scriptId });
    const loaded = Buffer.from(source.scriptSource);
    assetBytes += loaded.length;
    if (assetBytes > 64 * 1024 * 1024) throw Error('Asset evidence exceeded byte bound');
    const identity = assetIdentity(event.url, loaded, origin);
    bytes(`loaded-${identity.sha256}.js`, loaded);
    const response = await fetch(event.url, { signal, redirect: 'error', headers: { Authorization: `Bearer ${process.env.CODETERM_API_TOKEN}` } });
    if (!response.ok) throw Error(`Served asset HTTP ${response.status}`);
    const chunks = [];
    for await (const chunk of response.body) {
      assetBytes += chunk.length;
      if (assetBytes > 64 * 1024 * 1024) throw Error('Served asset evidence exceeded byte bound');
      chunks.push(chunk);
    }
    const served = Buffer.concat(chunks);
    const servedIdentity = assetIdentity(event.url, served, 'HTTP served at capture');
    bytes(`served-${servedIdentity.sha256}.js`, served);
    result.assets.push({ loaded: identity, served: servedIdentity, sameContent: identity.sha256 === servedIdentity.sha256 });
  };
  cdp.on('Debugger.scriptParsed', event => {
    if (event.url.startsWith('http://127.0.0.1:7686/') && /\.js(?:\?|$)/.test(event.url)) track(recordAsset(event, 'CDP Debugger.getScriptSource'));
  });
  cdp.on('Runtime.consoleAPICalled', event => {
    if (result.console.length >= 4096) { control.abort(Error('Console capture exceeded record bound')); return; }
    result.console.push({ type: event.type, at: event.timestamp });
  });
  await cdp.send('Runtime.enable');
  await cdp.send('Debugger.enable');
  while (jobs.size) await Promise.all([...jobs]);
  result.before = await cdp.evaluate(`(${snapshotOutputEvidence.toString()})()`);
  const diagnosticRequests = new Map();
  let wireReady, diagnosticReady;
  const hasWire = new Promise(resolve => { wireReady = resolve; });
  const hasDiagnostic = new Promise(resolve => { diagnosticReady = resolve; });
  const append = (id, encoded) => {
    try { wire.add(id, encoded); if (wire.bytes) wireReady(); }
    catch (error) { control.abort(error); }
  };
  cdp.on('Network.requestWillBeSent', event => {
    if (new URL(event.request.url).pathname === '/api/frontend/diagnostic') {
      if (diagnosticRequests.size >= 256) { control.abort(Error('Diagnostic capture exceeded record bound')); return; }
      let body;
      try { body = JSON.parse(event.request.postData); } catch { body = {}; }
      const { stage, errorName, reason, httpStatus, source, native, injected, mounted } = body;
      diagnosticRequests.set(event.requestId, { body: { stage, errorName, reason, httpStatus, source, native, injected, mounted } });
    }
  });
  cdp.on('Network.responseReceived', event => {
    const path = new URL(event.response.url).pathname;
    if (['/api/output/stream', '/api/events/stream'].includes(path)) {
      streams.set(event.requestId, { id: event.requestId, url: safeUrl(event.response.url), status: event.response.status });
      wire.arm(event.requestId);
      track(cdp.send('Network.streamResourceContent', { requestId: event.requestId }).then(reply => {
        wire.begin(event.requestId, reply.bufferedData);
        if (wire.bytes) wireReady();
      }));
    }
    if (diagnosticRequests.has(event.requestId)) diagnosticRequests.get(event.requestId).status = event.response.status;
  });
  cdp.on('Network.dataReceived', event => { if (streams.has(event.requestId) && event.data) append(event.requestId, event.data); });
  cdp.on('Network.loadingFailed', event => {
    if (streams.has(event.requestId)) { result.failures.push(`Stream failed: ${event.requestId}`); control.abort(Error('Captured stream failed')); }
  });
  cdp.on('Network.loadingFinished', event => {
    const record = diagnosticRequests.get(event.requestId);
    if (record) track(cdp.send('Network.getResponseBody', { requestId: event.requestId }).then(reply => {
      const text = reply.base64Encoded ? Buffer.from(reply.body, 'base64').toString() : reply.body;
      const response = JSON.parse(text);
      record.response = { ok: response.ok };
      result.diagnostics.push(record);
      diagnosticReady();
    }));
  });
  await cdp.send('Network.enable');
  await cdp.send('Page.enable');
  await cdp.send('Page.reload', { ignoreCache: false });
  await Promise.all([hasWire, hasDiagnostic]);
  const tab = JSON.stringify(args['--tab']);
  await cdp.evaluate(`new Promise(resolve => {
    const check = () => [...document.querySelectorAll('*')].some(e => e.__codetermTerminal && e.closest('[data-tab-id]')?.getAttribute('data-tab-id') === ${tab});
    const observer = new MutationObserver(() => { if (check()) { observer.disconnect(); resolve(true); } });
    observer.observe(document.documentElement, {subtree:true, childList:true, attributes:true});
    if (check()) { observer.disconnect(); resolve(true); }
  })`);
  result.after = await cdp.evaluate(`(${snapshotOutputEvidence.toString()})()`);
  while (jobs.size) await Promise.all([...jobs]);
  if (!result.assets.length || !result.after.terminals.length || !wire.bytes) throw Error('Missing capture surface');
  result.captureStatus = 'captured';
  result.diagnosticVerdict = diagnosticVerdict(result.diagnostics);
  if (result.diagnosticVerdict.status !== 'pass') throw Error('Shipped diagnostic acknowledgement failed');
  const final = await devEvidence(args['--expect-build'], signal);
  assertSameListener(result.listener, final.listener);
  result.finalListener = final.listener;
} });
