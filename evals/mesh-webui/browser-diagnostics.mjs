import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { waitOutputReadiness, installOutputParseProbe } from './output-readiness.mjs';

const streamPaths = new Set(['/api/mesh/relay-stream', '/api/output/stream']);

export function redactDiagnostic(value, secrets = []) {
  let text = String(value ?? '');
  for (const secret of secrets.filter(Boolean)) {
    for (const variant of new Set([secret, encodeURIComponent(secret)])) text = text.split(variant).join('[REDACTED]');
  }
  return text.replace(/([?&](?:token|access_token|api_token|api_key|authorization|secret)=)[^&#\s]*/gi, '$1[REDACTED]');
}

export function diagnosticRecord(event, payload, timestamp, secrets = []) {
  const clean = value => redactDiagnostic(value, secrets);
  if (['request', 'response', 'requestfinished', 'requestfailed'].includes(event)) {
    const url = new URL(payload.url);
    if (!streamPaths.has(url.pathname)) return null;
    const record = { event, timestamp, requestId: payload.requestId, url: clean(url.href), method: payload.method };
    if (event === 'requestfailed') record.failure = clean(payload.failure);
    if (event === 'response') record.status = payload.status;
    return record;
  }
  if (event.startsWith('network-')) {
    const url = new URL(payload.url);
    if (!streamPaths.has(url.pathname)) return null;
    return { event, timestamp, ...payload, url: clean(url.href) };
  }
  if (event === 'write-parsed') return { event, timestamp, ...payload };
  if (event === 'console') return { event, timestamp, level: payload.level, text: clean(payload.text),
    location: { url: clean(payload.location?.url), lineNumber: payload.location?.lineNumber,
      columnNumber: payload.location?.columnNumber } };
  if (event === 'pageerror') return { event, timestamp, name: clean(payload.name),
    message: clean(payload.message), stack: clean(payload.stack) };
  return null;
}

export function pendingStreamRequests(records) {
  const pending = new Map();
  for (const record of records) {
    if (record.event === 'request') pending.set(record.requestId, { ...record, state: 'pending' });
    if (record.event === 'response' && pending.has(record.requestId)) pending.get(record.requestId).responseStatus = record.status;
    if (['requestfinished', 'requestfailed'].includes(record.event)) pending.delete(record.requestId);
  }
  return [...pending.values()];
}

export async function armBrowserDiagnostics(page, directory, { secrets = [], outputTabId, now = () => new Date().toISOString() } = {}) {
  const records = [];
  const subscribers = new Set();
  let readinessProof;
  let session;
  const networkListeners = [];
  const requestIds = new WeakMap();
  const listeners = [];
  let sequence = 0;
  const identity = request => {
    if (!requestIds.has(request)) requestIds.set(request, ++sequence);
    return { requestId: requestIds.get(request), url: request.url(), method: request.method() };
  };
  const record = (event, payload) => {
    const mapped = diagnosticRecord(event, payload, now(), secrets);
    if (mapped) { records.push(mapped); for (const notify of [...subscribers]) notify(); }
  };
  const listen = (event, fn) => { page.on(event, fn); listeners.push([event, fn]); };
  listen('request', request => record('request', identity(request)));
  listen('requestfinished', request => record('requestfinished', identity(request)));
  listen('requestfailed', request => record('requestfailed', { ...identity(request), failure: request.failure()?.errorText }));
  listen('response', response => record('response', { ...identity(response.request()), status: response.status() }));
  listen('console', message => record('console', { level: message.type(), text: message.text(), location: message.location() }));
  listen('pageerror', error => record('pageerror', { name: error.name, message: error.message, stack: error.stack }));
  if (outputTabId) {
    await page.exposeFunction('__b3OutputParsed', payload => record('write-parsed', payload));
    await page.addInitScript(installOutputParseProbe, { tabId: outputTabId });
    session = await page.context().newCDPSession(page);
    const networkUrls = new Map();
    const networkListen = (event, fn) => { session.on(event, fn); networkListeners.push([event, fn]); };
    networkListen('Network.requestWillBeSent', event => {
      networkUrls.set(event.requestId, event.request.url);
      record('network-request', { networkRequestId: event.requestId, url: event.request.url, method: event.request.method });
    });
    networkListen('Network.responseReceived', event => record('network-response', {
      networkRequestId: event.requestId, url: event.response.url, status: event.response.status }));
    networkListen('Network.dataReceived', event => {
      const url = networkUrls.get(event.requestId);
      if (url) record('network-bytes', { networkRequestId: event.requestId, url,
        dataLength: event.dataLength, encodedDataLength: event.encodedDataLength });
    });
    await session.send('Network.enable');
  }
  return { async waitForOutput(target, homeProof) {
    const started = now();
    try {
      const state = await waitOutputReadiness({ records: () => records,
        subscribe: notify => { subscribers.add(notify); return () => subscribers.delete(notify); },
        target, homeProof, signal: AbortSignal.timeout(30000) });
      readinessProof = { started, at: now(), status: 'READY', ...state };
      return readinessProof;
    } catch (error) {
      readinessProof = { started, at: now(), status: 'FAIL no output within 30 s', ...error.outputState };
      throw error;
    }
  }, async finish(reason = 'case-complete') {
    for (const [event, fn] of listeners) page.off(event, fn);
    for (const [event, fn] of networkListeners) session.off(event, fn);
    const pending = pendingStreamRequests(records);
    records.push({ event: 'capture-boundary', timestamp: now(), reason, pendingRequestIds: pending.map(item => item.requestId) });
    await mkdir(directory, { recursive: true });
    if (readinessProof) await writeFile(path.join(directory, 'output-readiness.json'), JSON.stringify(readinessProof, null, 2), { flag: 'wx' });
    await writeFile(path.join(directory, 'browser-events.jsonl'), records.map(item => JSON.stringify(item)).join('\n') + '\n', { flag: 'wx' });
    await writeFile(path.join(directory, 'browser-requests.json'), JSON.stringify({
      boundary: 'before context teardown', reason,
      initiated: records.filter(item => item.event === 'request').length, pending,
      finished: records.filter(item => item.event === 'requestfinished'),
      failed: records.filter(item => item.event === 'requestfailed'),
    }, null, 2), { flag: 'wx' });
  } };
}
