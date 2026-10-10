import { spawn } from 'node:child_process';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { once } from 'node:events';

export async function openBrowser(outputDir) {
  await mkdir(outputDir, { recursive: true });
  const executable = process.env.EDGE_PATH ?? 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
  const browser = spawn(executable, ['--headless=new', '--no-first-run', '--no-default-browser-check', '--disable-background-networking', '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--remote-debugging-port=0', `--user-data-dir=${join(outputDir, 'edge-profile')}`, 'about:blank'], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
  const address = await new Promise((resolve, reject) => {
    let output = '';
    browser.stderr.on('data', chunk => {
      output += chunk;
      const match = output.match(/DevTools listening on (ws:\/\/[^\s]+)/);
      if (match) resolve(match[1]);
    });
    browser.once('error', reject);
    browser.once('exit', code => reject(new Error(`Edge exited before readiness: ${code}`)));
  });
  const socket = new WebSocket(address);
  await once(socket, 'open');
  let sequence = 0;
  const pending = new Map();
  const listeners = new Map();
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    if (message.id) {
      const promise = pending.get(message.id);
      if (!promise) return;
      pending.delete(message.id);
      if (message.error) promise.reject(new Error(JSON.stringify(message.error)));
      else promise.resolve(message.result);
    } else for (const handler of listeners.get(message.method) ?? []) handler(message.params);
  });
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params, sessionId }));
  });
  const on = (method, handler) => {
    const handlers = listeners.get(method) ?? new Set();
    handlers.add(handler);
    listeners.set(method, handlers);
    return () => handlers.delete(handler);
  };
  const next = method => new Promise(resolve => {
    const dispose = on(method, value => { dispose(); resolve(value); });
  });
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const command = (method, params) => send(method, params, sessionId);
  await command('Page.enable');
  await command('Runtime.enable');
  const evaluate = async expression => {
    const result = await command('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  const navigate = async url => {
    const loaded = next('Page.loadEventFired');
    const result = await command('Page.navigate', { url });
    if (result.errorText) throw new Error(result.errorText);
    await loaded;
    await evaluate('Promise.all([document.fonts.ready, ...Array.from(document.images, image => image.decode())])');
    await evaluate('new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))');
  };
  const close = async () => {
    await send('Browser.close').catch(() => {});
    socket.close();
  };
  return { command, evaluate, navigate, on, close };
}
