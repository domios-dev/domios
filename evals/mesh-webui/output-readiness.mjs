export function matchesOutputRequest(url, { origin, peerId, tabId }) {
  const u = new URL(url);
  if (u.origin !== origin) return false;
  if (peerId) return u.pathname === '/api/mesh/relay-stream'
    && u.searchParams.get('peer') === peerId && u.searchParams.get('path') === '/api/output/stream'
    && u.searchParams.get('tab') === `mesh:${peerId}/${tabId}`;
  if (u.pathname !== '/api/output/stream') return false;
  if (u.searchParams.get('tab') === tabId) return true;
  try {
    const cursor = JSON.parse(Buffer.from(u.searchParams.get('since'), 'base64url').toString());
    const ids = Object.keys(cursor.tabs ?? {});
    return ids.length === 1 && ids[0] === tabId;
  } catch { return false; }
}

export function outputReadinessState(records, target, homeProof) {
  const requests = new Map();
  const network = new Map();
  const parsedEvents = [];
  for (const [index, row] of records.entries()) {
    if (row.event === 'write-parsed' && row.tabId === target.tabId && row.nonEmpty) parsedEvents.push({ ...row, index });
    if (!row.url || !matchesOutputRequest(row.url, target)) continue;
    if (row.event === 'request') requests.set(row.requestId, { requestId: row.requestId, url: row.url,
      initiated: row.timestamp, headers: null, terminal: null });
    if (row.event === 'response' && requests.has(row.requestId)) requests.get(row.requestId).headers = { timestamp: row.timestamp, status: row.status, index };
    if (['requestfinished', 'requestfailed'].includes(row.event) && requests.has(row.requestId)) requests.get(row.requestId).terminal = row;
    if (row.event === 'network-request') network.set(row.networkRequestId, { networkRequestId: row.networkRequestId,
      url: row.url, initiated: row.timestamp, headers: null, receivedBytes: 0, encodedBytes: 0 });
    if (row.event === 'network-response' && network.has(row.networkRequestId)) network.get(row.networkRequestId).headers = { timestamp: row.timestamp, status: row.status };
    if (row.event === 'network-bytes' && network.has(row.networkRequestId)) {
      const request = network.get(row.networkRequestId);
      request.receivedBytes += row.dataLength;
      request.encodedBytes += row.encodedDataLength;
    }
  }
  const accepted = [...requests.values()].filter(row => row.headers?.status === 200 && row.terminal?.event !== 'requestfailed');
  const parsed = parsedEvents.find(event => accepted.some(row => event.index > row.headers.index)) ?? null;
  const headers = accepted.length > 0;
  const homeEmpty = homeProof?.tab_id === target.tabId && homeProof?.answered_by?.instance_id === target.homePeerId
    && typeof homeProof.text === 'string' && homeProof.text.trim() === '';
  return { target, requests: [...requests.values()], network: [...network.values()],
    parsed, homeEmpty, homeProof, ready: headers && (Boolean(parsed) || homeEmpty) };
}

export function waitOutputReadiness({ records, subscribe, target, homeProof, signal }) {
  return new Promise((resolve, reject) => {
    let unsubscribe = () => {};
    let settled = false;
    const cleanup = () => { unsubscribe(); signal.removeEventListener('abort', expired); };
    const expired = () => {
      if (settled) return;
      settled = true;
      cleanup();
      const error = new Error('FAIL no output within 30 s');
      error.outputState = outputReadinessState(records(), target, homeProof);
      reject(error);
    };
    const changed = () => {
      if (settled) return;
      const state = outputReadinessState(records(), target, homeProof);
      if (!state.ready) return;
      settled = true;
      cleanup();
      resolve(state);
    };
    unsubscribe = subscribe(changed);
    signal.addEventListener('abort', expired, { once: true });
    if (signal.aborted) expired();
    else changed();
  });
}

export function installOutputParseProbe({ tabId }) {
  const disposables = [];
  const instrumented = new WeakSet();
  const emit = term => {
    const buffer = term.buffer.active;
    let nonEmpty = false;
    for (let y = 0; y < buffer.length; y++) {
      if (buffer.getLine(y)?.translateToString(true).trim()) { nonEmpty = true; break; }
    }
    void window.__b3OutputParsed({ tabId, nonEmpty, buffer: buffer.type, at: performance.now() });
  };
  const attach = term => {
    if (!term) return;
    disposables.push(term.onWriteParsed(() => emit(term)));
  };
  const scan = () => {
    for (const root of document.querySelectorAll('[data-testid="terminal-tab"]')) {
      if (root.dataset.tabId !== tabId) continue;
      const host = root.querySelector('[data-testid="terminal-xterm-host"]');
      if (!host || instrumented.has(host)) continue;
      instrumented.add(host);
      const descriptor = Object.getOwnPropertyDescriptor(host, '__codetermTerminal');
      if (descriptor) { attach(host.__codetermTerminal); continue; }
      let term;
      Object.defineProperty(host, '__codetermTerminal', { configurable: true, enumerable: true,
        get: () => term, set: value => { term = value; attach(value); } });
    }
  };
  const observer = new MutationObserver(scan);
  observer.observe(document, { childList: true, subtree: true });
  scan();
  window.__b3OutputParseProbe = { dispose: () => { observer.disconnect(); for (const handle of disposables) handle.dispose(); } };
}
