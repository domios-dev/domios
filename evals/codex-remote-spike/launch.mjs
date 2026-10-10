import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const options = Object.fromEntries(process.argv.slice(2).reduce((out, value, index, all) => {
  if (value.startsWith('--')) out.push([value.slice(2), all[index + 1]]);
  return out;
}, []));
const binary = options.binary;
const dir = options.dir;
const port = Number(options.port);
if (!binary || !path.isAbsolute(binary) || /\.(cmd|bat)$/i.test(binary)) throw new Error('Supply the absolute native Codex binary');
if (!dir || !path.isAbsolute(dir) || !Number.isInteger(port) || port < 1024 || port > 65535 || [7685, 7686].includes(port)) {
  throw new Error('Supply an absolute fresh --dir and dedicated --port');
}
if (!process.env.CODETERM_TAB_ID || !process.env.CODETERM_SESSION_BINDING_NONCE) throw new Error('Run inside the authorized Domios utility tab');
fs.mkdirSync(dir, { recursive: true });
fs.writeFileSync(path.join(dir, 'launcher-pid'), String(process.pid));
const tokenFile = path.join(dir, 'token');
const token = crypto.randomBytes(32).toString('hex');
fs.writeFileSync(tokenFile, token, { flag: 'wx', mode: 0o600 });
const env = { ...process.env, CODETERM_CODEX_RPC_TOKEN: token };
const children = new Set();
let stopping = false;
function child(executable, args, stdio) {
  const processChild = spawn(executable, args, { cwd: dir, env, stdio, windowsHide: true });
  children.add(processChild);
  processChild.on('exit', () => children.delete(processChild));
  processChild.on('error', error => { console.error(error); stop(1); });
  return processChild;
}
function stop(code) {
  if (stopping) return;
  stopping = true;
  process.exitCode = code;
  for (const processChild of children) processChild.kill();
  fs.rmSync(tokenFile, { force: true });
}
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => stop(1));
fs.writeFileSync(path.join(dir, 'launch.json'), JSON.stringify({
  binary, port, tabId: env.CODETERM_TAB_ID, nonce: env.CODETERM_SESSION_BINDING_NONCE,
  apiUrl: env.CODETERM_API_URL, codexHomeOverride: env.CODEX_HOME ?? null,
}, null, 2));
const server = child(binary, ['app-server', '--listen', `ws://127.0.0.1:${port}`,
  '--ws-auth', 'capability-token', '--ws-token-file', tokenFile], ['ignore', 'pipe', 'pipe']);
fs.writeFileSync(path.join(dir, 'server-pid'), String(server.pid));
const log = fs.createWriteStream(path.join(dir, 'server.log'), { flags: 'wx' });
let mint;
let launched = false;
const deadline = AbortSignal.timeout(30000);
deadline.addEventListener('abort', () => {
  if (!launched && !stopping) { console.error('Launch readiness deadline'); stop(1); }
}, { once: true });
for (const stream of [server.stdout, server.stderr]) {
  const lines = readline.createInterface({ input: stream });
  lines.on('line', line => {
    log.write(`${line}\n`);
    console.log(line);
    if (mint || !line.includes(`listening on: ws://127.0.0.1:${port}`)) return;
    const probe = fileURLToPath(new URL('./probe.mjs', import.meta.url));
    const flow = options.flow ?? 'empty';
    const flowArgs = flow === 'tui' ? ['--follow'] : ['--mint', ...(flow === 'seed' ? ['--seed'] : flow === 'inject' ? ['--inject'] : [])];
    mint = child(process.execPath, [probe, '--port', String(port), '--token-file', tokenFile,
      ...flowArgs, '--interactive', '--cwd', dir, '--thread-file', path.join(dir, 'thread'),
      '--out', path.join(dir, 'mint.ndjson')], ['pipe', 'pipe', 'inherit']);
    mint.on('exit', code => { if (!stopping) stop(code || 1); });
    readline.createInterface({ input: mint.stdout }).on('line', message => {
      const ready = JSON.parse(message);
      if (!ready.ready || launched) return;
      launched = true;
      console.log('MINTED_THREAD', ready.threadId);
      const tui = child(binary, ['-c', 'check_for_update_on_startup=false', '--remote', `ws://127.0.0.1:${port}`,
        '--remote-auth-token-env', 'CODETERM_CODEX_RPC_TOKEN', ...(flow === 'tui' ? [] : ['resume', ready.threadId])], 'inherit');
      fs.writeFileSync(path.join(dir, 'tui-pid'), String(tui.pid));
      tui.on('exit', code => { console.log('TUI_EXIT', code); stop(code ?? 1); });
    });
  });
}
server.on('exit', code => { log.end(); console.log('SERVER_EXIT', code); if (!stopping) stop(1); });
