import { readFileSync, writeFileSync } from 'node:fs';

const args = process.argv.slice(2);
const opt = name => args[args.indexOf(name) + 1];
for (const name of ['--endpoint-file', '--url', '--tab', '--expression', '--out']) {
  if (!args.includes(name)) throw Error(`Missing ${name}`);
}
if (new URL(process.env.CODETERM_API_URL || '').origin !== 'http://127.0.0.1:7686') {
  throw Error('DEV wrapper required');
}
const endpoint = readFileSync(opt('--endpoint-file'), 'utf8').trim();
const browser = new URL(endpoint);
const page = new URL(opt('--url'));
if (browser.protocol !== 'ws:' || browser.hostname !== '127.0.0.1' || !browser.port) {
  throw Error('Local Browser endpoint required');
}
if (page.protocol !== 'http:' || page.hostname !== '127.0.0.1' || !page.port ||
    page.port === '7685' || page.search || page.hash) {
  throw Error('Owned nonsecret DEV/fixture page required');
}
const deadline = AbortSignal.timeout(15000);
const ws = new WebSocket(endpoint);
const pending = new Map();
let nextId = 0;
function failPending() {
  for (const request of pending.values()) request.reject(Error('CDP connection ended'));
  pending.clear();
}
deadline.addEventListener('abort', () => { failPending(); ws.close(); }, { once: true });
ws.addEventListener('close', failPending);
await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve, { once: true });
  ws.addEventListener('error', () => reject(Error('Owned Browser connection failed')), { once: true });
  deadline.addEventListener('abort', () => reject(Error('CDP connection deadline')), { once: true });
});
ws.onmessage = event => {
  const message = JSON.parse(event.data);
  const request = pending.get(message.id);
  if (!request) return;
  pending.delete(message.id);
  if (message.error) request.reject(Error('CDP command refused'));
  else request.resolve(message.result);
};
function call(method, params = {}, sessionId) {
  if (deadline.aborted) return Promise.reject(Error('CDP deadline'));
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
}
try {
  const { targetInfos } = await call('Target.getTargets');
  const matches = targetInfos.filter(target => target.type === 'page' && target.url === page.href);
  if (matches.length !== 1) throw Error('Expected exactly one owned page target');
  const { sessionId } = await call('Target.attachToTarget', {
    targetId: matches[0].targetId, flatten: true,
  });
  const context = {
    tabId: opt('--tab'), phase: args.includes('--phase') ? opt('--phase') : 'read',
  };
  const source = readFileSync(opt('--expression'), 'utf8').replace(/^\uFEFF/, '');
  const result = await call('Runtime.evaluate', {
    expression: `(${source})(Object.freeze(${JSON.stringify(context)}))`,
    returnByValue: true, awaitPromise: true,
  }, sessionId);
  if (result.exceptionDetails) throw Error('Owned-page evaluation failed');
  const evidence = { target_id: matches[0].targetId, result: result.result.value };
  writeFileSync(opt('--out'), JSON.stringify(evidence, null, 2), { flag: 'wx' });
  console.log(JSON.stringify(evidence));
} finally {
  ws.close();
}
