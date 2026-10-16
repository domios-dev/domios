import { mkdirSync, writeFileSync, existsSync, readdirSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { listenerIdentity } from '../core-journeys/lib/listener.mjs';
import { client } from '../core-journeys/lib/env.mjs';
import { assertDevHealth } from './preflight.mjs';
import { safeUrl } from './evidence-model.mjs';

export function options(argv = process.argv.slice(2)) {
  const values = {};
  const allowed = new Set(['--expect-build', '--out', '--cdp', '--target', '--tab', '--fixture', '--dry-run', '--reload', '--force-exit']);
  for (let i = 0; i < argv.length; i++) {
    const key = argv[i];
    if (!allowed.has(key) || key in values) throw Error(`Invalid or repeated option ${key}`);
    values[key] = ['--dry-run', '--reload', '--force-exit'].includes(key) ? true : argv[++i];
    if (values[key] === undefined || String(values[key]).startsWith('--')) throw Error(`Missing value for ${key}`);
  }
  if (!values['--expect-build'] || !values['--out']) throw Error('--expect-build SHA --out DIR required');
  return values;
}

export function artifacts(destination) {
  const out = resolve(destination);
  if (existsSync(out) && readdirSync(out).length) throw Error('Fresh empty artifact directory required');
  mkdirSync(out, { recursive: true });
  return { out, save: (name, data) => writeFileSync(join(out, name), JSON.stringify(data, null, 2)),
    bytes: (name, data) => writeFileSync(join(out, name), data) };
}

export async function devEvidence(expected, signal) {
  if (process.env.CODETERM_API_PORT !== '7686' || !process.env.CODETERM_DEV_DATA_DIR) throw Error('Run through evidence.sh / dev_run');
  const { api } = client({ url: 'http://127.0.0.1:7686', token: process.env.CODETERM_API_TOKEN });
  const health = await api('/api/health', { signal });
  assertDevHealth(health);
  if (health.build_sha !== expected) throw Error('Unexpected DEV binary SHA');
  return { api, health, listener: listenerIdentity(7686) };
}

export function assertSameListener(before, after) {
  if (before.pid !== after.pid || before.started_us !== after.started_us) throw Error('DEV listener changed during capture');
}

export function loopback(value) {
  const url = new URL(value);
  if (!['http:', 'ws:'].includes(url.protocol) || url.hostname !== '127.0.0.1' || url.username || url.password || url.search || url.hash)
    throw Error('Explicit plain loopback endpoint required');
  return url;
}

export class Cdp {
  constructor(socket, signal) {
    this.socket = socket; this.signal = signal; this.pending = new Map(); this.listeners = new Map(); this.id = 0;
    socket.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      if (message.id) {
        const operation = this.pending.get(message.id);
        this.pending.delete(message.id);
        if (message.error) operation?.reject(Error(message.error.message));
        else operation?.resolve(message.result);
      } else for (const handler of this.listeners.get(message.method) ?? []) handler(message.params);
    });
    const failed = () => { for (const operation of this.pending.values()) operation.reject(Error('CDP closed')); this.pending.clear(); };
    socket.addEventListener('close', failed);
    socket.addEventListener('error', failed);
    this.aborted = () => { failed(); socket.close(); };
    signal.addEventListener('abort', this.aborted, { once: true });
    if (signal.aborted) this.aborted();
  }
  static async connect(endpoint, targetId, signal) {
    const origin = loopback(endpoint);
    const response = await fetch(new URL('/json', origin), { signal });
    if (!response.ok) throw Error(`CDP targets HTTP ${response.status}`);
    const targets = await response.json();
    const target = targets.find(value => value.id === targetId && value.type === 'page');
    if (!target || new URL(target.url).origin !== 'http://127.0.0.1:7686') throw Error('Exact owned DEV Browser target required');
    const ws = loopback(target.webSocketDebuggerUrl);
    if (ws.host !== origin.host) throw Error('CDP websocket origin mismatch');
    const socket = new WebSocket(ws);
    await new Promise((resolve, reject) => {
      const abort = () => { socket.close(); reject(signal.reason); };
      signal.addEventListener('abort', abort, { once: true });
      socket.addEventListener('open', () => { signal.removeEventListener('abort', abort); resolve(); }, { once: true });
      socket.addEventListener('error', () => { signal.removeEventListener('abort', abort); reject(Error('CDP connection failed')); }, { once: true });
      if (signal.aborted) abort();
    });
    const cdp = new Cdp(socket, signal);
    cdp.target = { id: target.id, url: safeUrl(target.url) };
    return cdp;
  }
  on(method, handler) {
    if (!this.listeners.has(method)) this.listeners.set(method, new Set());
    this.listeners.get(method).add(handler);
    return () => this.listeners.get(method)?.delete(handler);
  }
  send(method, params = {}) {
    this.signal.throwIfAborted();
    return new Promise((resolve, reject) => {
      const id = ++this.id;
      this.pending.set(id, { resolve, reject });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }
  async evaluate(expression) {
    const reply = await this.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (reply.exceptionDetails) throw Error('In-page evidence evaluation failed');
    return reply.result.value;
  }
  close() { this.signal.removeEventListener('abort', this.aborted); this.aborted(); }
}
