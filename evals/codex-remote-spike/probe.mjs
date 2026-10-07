import net from 'node:net';
import crypto from 'node:crypto';
import fs from 'node:fs';
import readline from 'node:readline';
import { EventEmitter, once } from 'node:events';

const args = Object.fromEntries(process.argv.slice(2).reduce((pairs, arg, i, all) => {
  if (arg.startsWith('--')) pairs.push([arg.slice(2), all[i + 1]?.startsWith('--') ? true : all[i + 1] ?? true]);
  return pairs;
}, []));
const required = name => {
  if (!args[name]) throw new Error(`Missing --${name}`);
  return args[name];
};
const port = Number(required('port'));
if (!Number.isInteger(port) || port < 1024 || port > 65535 || port === 7685 || port === 7686) {
  throw new Error('Use a dedicated localhost port above 1023, excluding Domios ports');
}
const token = fs.readFileSync(required('token-file'), 'utf8').trim();
const output = args.out ? fs.createWriteStream(args.out, { flags: 'wx' }) : process.stdout;
function record(kind, value) {
  output.write(`${JSON.stringify({ at: new Date().toISOString(), kind, value })}\n`);
}

class Wire extends EventEmitter {
  buffer = Buffer.alloc(0);
  fragments = [];
  upgraded = false;
  constructor() {
    super();
    this.key = crypto.randomBytes(16).toString('base64');
    this.socket = net.connect({ host: '127.0.0.1', port });
    this.socket.on('connect', () => this.socket.write([
      'GET / HTTP/1.1', `Host: 127.0.0.1:${port}`, 'Upgrade: websocket',
      'Connection: Upgrade', `Sec-WebSocket-Key: ${this.key}`, 'Sec-WebSocket-Version: 13',
      `Authorization: Bearer ${token}`, '', '',
    ].join('\r\n')));
    this.socket.on('data', data => this.consume(data));
    this.socket.on('error', error => this.emit('error', error));
    this.socket.on('close', () => this.emit('close'));
  }
  consume(data) {
    this.buffer = Buffer.concat([this.buffer, data]);
    if (!this.upgraded) {
      const end = this.buffer.indexOf('\r\n\r\n');
      if (end < 0) return;
      const headers = this.buffer.subarray(0, end).toString();
      const accept = crypto.createHash('sha1').update(this.key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
      if (!headers.startsWith('HTTP/1.1 101') || !headers.toLowerCase().includes(accept.toLowerCase())) {
        this.emit('error', new Error(`WebSocket upgrade failed: ${headers.split('\r\n')[0]}`));
        this.socket.destroy();
        return;
      }
      this.upgraded = true;
      this.buffer = this.buffer.subarray(end + 4);
      this.emit('open');
    }
    while (this.buffer.length >= 2) {
      const first = this.buffer[0];
      const opcode = first & 15;
      let size = this.buffer[1] & 127;
      let offset = 2;
      if (size === 126) {
        if (this.buffer.length < 4) return;
        size = this.buffer.readUInt16BE(2);
        offset = 4;
      } else if (size === 127) {
        if (this.buffer.length < 10) return;
        size = Number(this.buffer.readBigUInt64BE(2));
        offset = 10;
      }
      if (this.buffer[1] & 128 || size > 64 * 1024 * 1024) throw new Error('Invalid server frame');
      if (this.buffer.length < offset + size) return;
      const payload = this.buffer.subarray(offset, offset + size);
      this.buffer = this.buffer.subarray(offset + size);
      if (opcode === 8) { this.socket.end(); return; }
      if (opcode === 9) { this.frame(payload, 10); continue; }
      if (opcode === 10) continue;
      if (opcode !== 0 && opcode !== 1) throw new Error('Expected text frame');
      this.fragments.push(payload);
      if (first & 128) {
        this.emit('message', JSON.parse(Buffer.concat(this.fragments).toString('utf8')));
        this.fragments = [];
      }
    }
  }
  frame(payload, opcode = 1) {
    const mask = crypto.randomBytes(4);
    const extra = payload.length < 126 ? 0 : payload.length < 65536 ? 2 : 8;
    const header = Buffer.alloc(2 + extra + 4);
    header[0] = 128 | opcode;
    header[1] = 128 | (extra === 0 ? payload.length : extra === 2 ? 126 : 127);
    if (extra === 2) header.writeUInt16BE(payload.length, 2);
    if (extra === 8) header.writeBigUInt64BE(BigInt(payload.length), 2);
    mask.copy(header, 2 + extra);
    const masked = Buffer.from(payload);
    for (let i = 0; i < masked.length; i++) masked[i] ^= mask[i % 4];
    this.socket.write(Buffer.concat([header, masked]));
  }
  send(value) { record('send', value); this.frame(Buffer.from(JSON.stringify(value))); }
}

const wire = new Wire();
const pending = new Map();
const received = [];
let nextId = 1000;
let threadId = args.thread;
wire.on('message', value => {
  record('receive', value);
  received.push(value);
  if (args.follow && !threadId && value.method === 'thread/started' && !value.params.thread.parentThreadId) {
    threadId = value.params.thread.id;
    rpc('thread/resume', { threadId, excludeTurns: true }).then(
      result => record('joined', { threadId, result }),
      error => record('joinError', { threadId, error: error.message }),
    );
  }
  if (value.method) return;
  const callback = pending.get(value.id);
  if (callback) { pending.delete(value.id); callback(value); }
});
wire.on('error', error => { record('error', error.message); process.exitCode = 1; });
wire.on('close', () => {
  for (const callback of pending.values()) callback({ error: { message: 'Socket closed' } });
  pending.clear();
});
await once(wire, 'open', { signal: AbortSignal.timeout(15000) });
async function rpc(method, params = {}) {
  const id = nextId++;
  const response = new Promise(resolve => pending.set(id, resolve));
  wire.send({ id, method, params });
  const result = await Promise.race([
    response,
    new Promise((_, reject) => {
      const signal = AbortSignal.timeout(30000);
      signal.addEventListener('abort', () => reject(new Error(`Deadline: ${method}`)), { once: true });
    }),
  ]);
  if (result.error) throw new Error(JSON.stringify(result.error));
  return result.result;
}
await rpc('initialize', { clientInfo: { name: 'domios_remote_spike', version: '1.0.0' }, capabilities: { experimentalApi: true } });
wire.send({ method: 'initialized' });
if (args.mint) {
  const result = await rpc('thread/start', {
    cwd: required('cwd'), ephemeral: false, approvalPolicy: 'on-request', approvalsReviewer: 'user',
    sandbox: 'read-only', developerInstructions: 'This is a verification spike. Follow the user test instructions precisely. Never modify global configuration or hooks.',
  });
  threadId = result.thread.id;
  if (args['thread-file']) fs.writeFileSync(args['thread-file'], `${threadId}\n`, { flag: 'wx' });
  record('minted', { threadId });
}
if (args.inject) {
  await rpc('thread/inject_items', { threadId, items: [{ type: 'message', role: 'developer',
    content: [{ type: 'input_text', text: 'Domios spike binding marker.' }] }] });
  record('materialized', { threadId, method: 'thread/inject_items' });
}
if (args.seed) {
  const turn = await rpc('turn/start', { threadId, clientUserMessageId: 'spike-seed',
    input: [{ type: 'text', text: 'Reply with SEED_READY only. Do not use tools.' }], approvalsReviewer: 'user' });
  const matches = frame => frame.method === 'turn/completed' && frame.params.threadId === threadId && frame.params.turn.id === turn.turn.id;
  let completed = received.find(matches);
  if (!completed) {
    completed = await new Promise((resolve, reject) => {
      const signal = AbortSignal.timeout(120000);
      const listener = frame => { if (matches(frame)) { wire.off('message', listener); resolve(frame); } };
      signal.addEventListener('abort', () => { wire.off('message', listener); reject(new Error('Seed turn deadline')); }, { once: true });
      wire.on('message', listener);
    });
  }
  record('seedCompleted', completed.params);
}
if (threadId && !args.mint) await rpc('thread/resume', { threadId, excludeTurns: true });
record('ready', { threadId, commands: 'One JSON object per stdin line: {method,params}; {replyTo,result}; {action:"backfill"}; {action:"quit"}' });
if (args.out) process.stdout.write(`${JSON.stringify({ ready: true, threadId })}\n`);

async function pages(method, params) {
  const data = [];
  let cursor;
  const seen = new Set();
  do {
    const page = await rpc(method, { ...params, limit: 100, ...(cursor ? { cursor } : {}) });
    data.push(...page.data);
    cursor = page.nextCursor;
    if (cursor && seen.has(cursor)) throw new Error('Repeated pagination cursor');
    seen.add(cursor);
  } while (cursor);
  return data;
}
async function backfill() {
  const turns = await pages('thread/turns/list', { threadId, sortDirection: 'asc' });
  const items = await pages('thread/items/list', { threadId, sortDirection: 'asc' });
  const repeat = await pages('thread/items/list', { threadId, sortDirection: 'asc' });
  const ids = items.map(item => item.id ?? item.item?.id);
  const stable = ids.length > 0 && ids.every(Boolean) && JSON.stringify(ids) === JSON.stringify(repeat.map(item => item.id ?? item.item?.id));
  const liveIds = received.filter(frame => frame.method === 'item/completed' && frame.params.threadId === threadId).map(frame => frame.params.item.id);
  record('backfill', { turns, items, stable, allObservedItemsPresent: liveIds.every(id => ids.includes(id)) });
}
async function commandLine(line) {
    if (!line.trim()) return;
    try {
      const command = JSON.parse(line);
      if (command.action === 'quit') return 'quit';
      if (command.action === 'backfill') { await backfill(); return; }
      if ('replyTo' in command) { wire.send({ id: command.replyTo, result: command.result }); return; }
      const result = await rpc(command.method, command.params ?? {});
      if (command.method === 'thread/resume') threadId = command.params.threadId;
      record('commandResult', { method: command.method, result });
    } catch (error) { record('commandError', error.message); }
}
if (args['control-file']) {
  await new Promise(resolve => {
    let consumed = 0;
    let queue = Promise.resolve();
    const watcher = fs.watch(args['control-file'], () => {
      queue = queue.then(async () => {
        const lines = fs.readFileSync(args['control-file'], 'utf8').split('\n');
        while (consumed < lines.length - 1) {
          if (await commandLine(lines[consumed++]) === 'quit') { watcher.close(); resolve(); return; }
        }
      });
    });
  });
} else if (args.interactive) {
  const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of input) if (await commandLine(line) === 'quit') break;
}
if (args.checklist) {
  // Protocol captures alone cannot prove a visible TUI or the two physical-machine gate.
  for (let item = 2; item <= 13; item++) record('checklist', { item, status: 'NOT OBSERVED', reason: 'Review captured frames together with the matching TUI, environment and teardown transcript; no automatic GO.' });
}
wire.frame(Buffer.alloc(0), 8);
wire.socket.end();
await once(wire, 'close');
if (output !== process.stdout) output.end();
