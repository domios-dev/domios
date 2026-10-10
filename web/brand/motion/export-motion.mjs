import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, writeFile } from 'node:fs/promises';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const directory = path.dirname(fileURLToPath(import.meta.url));
const work = path.resolve(directory, '../.render/round3-motion');
const output = process.argv[2];
if (!output) throw new Error('Pass an output directory in your Agent Pad.');
await mkdir(work, { recursive: true });
await mkdir(output, { recursive: true });
const browserPath = process.env.DOMIOS_EDGE_PATH ?? 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const browser = spawn(browserPath, [
  '--headless', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
  '--remote-debugging-port=0', `--user-data-dir=${path.join(work, 'profile')}`, 'about:blank',
], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
const lines = createInterface({ input: browser.stderr });
const endpoint = await new Promise((resolve, reject) => {
  const deadline = AbortSignal.timeout(60000);
  deadline.addEventListener('abort', () => reject(new Error('Browser endpoint deadline exceeded.')), { once: true });
  lines.on('line', line => {
    const match = line.match(/DevTools listening on (ws:\/\/\S+)/);
    if (match) resolve(match[1]);
  });
  browser.once('error', reject);
  browser.once('exit', code => reject(new Error(`Browser exited before endpoint: ${code}`)));
});
const socket = new WebSocket(endpoint);
await once(socket, 'open', { signal: AbortSignal.timeout(60000) });
let sequence = 0;
const pending = new Map();
const listeners = new Set();
socket.addEventListener('message', event => {
  const message = JSON.parse(event.data);
  if (message.id) {
    const recipient = pending.get(message.id);
    if (!recipient) return;
    pending.delete(message.id);
    if (message.error) recipient.reject(new Error(JSON.stringify(message.error)));
    else recipient.resolve(message.result);
  } else {
    for (const listener of listeners) listener(message);
  }
});
socket.addEventListener('close', () => {
  for (const recipient of pending.values()) recipient.reject(new Error('Browser connection closed.'));
  pending.clear();
});

function send(method, params = {}, sessionId) {
  return new Promise((resolve, reject) => {
    const id = ++sequence;
    const deadline = AbortSignal.timeout(60000);
    deadline.addEventListener('abort', () => {
      if (pending.delete(id)) reject(new Error(`${method} deadline exceeded.`));
    }, { once: true });
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
}

function event(method, sessionId) {
  return new Promise((resolve, reject) => {
    const listener = message => {
      if (message.method === method && message.sessionId === sessionId) {
        listeners.delete(listener);
        resolve(message.params);
      }
    };
    listeners.add(listener);
    AbortSignal.timeout(60000).addEventListener('abort', () => {
      if (listeners.delete(listener)) reject(new Error(`${method} event deadline exceeded.`));
    }, { once: true });
  });
}

try {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const call = (method, params) => send(method, params, sessionId);
  const evaluate = async expression => {
    const result = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  const motionPreference = async value => {
    await evaluate('window.motionPreferenceChanged = new Promise(resolve => matchMedia("(prefers-reduced-motion: reduce)").addEventListener("change", event => resolve(event.matches), {once: true})); true');
    await call('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value }] });
    await evaluate('window.motionPreferenceChanged');
  };
  const screenshot = async (file, width, height) => {
    const result = await call('Page.captureScreenshot', {
      format: 'png', captureBeyondViewport: true,
      clip: { x: 0, y: 0, width, height, scale: 1 },
    });
    await writeFile(file, Buffer.from(result.data, 'base64'));
  };
  await call('Page.enable');
  await call('Emulation.setDeviceMetricsOverride', { width: 1280, height: 1000, deviceScaleFactor: 1, mobile: false });
  const loaded = event('Page.loadEventFired', sessionId);
  await call('Page.navigate', { url: new URL('./index.html', import.meta.url).href });
  await loaded;
  await evaluate('window.logoMotion.seek("open", 2)');
  await screenshot(path.join(output, 'motion-prototype.png'), 1280, 1000);
  const normal = await evaluate('window.logoMotion.inspect()');
  await motionPreference('reduce');
  const reduced = {};
  for (const kind of ['open', 'arrival', 'breathe', 'press']) {
    reduced[kind] = await evaluate(`window.logoMotion.seek(${JSON.stringify(kind)}, .2); window.logoMotion.inspect()`);
  }
  await screenshot(path.join(output, 'reduced-motion.png'), 1280, 1000);
  if (Object.values(reduced).some(state => !state.reduced || state.running || state.path !== normal.path || state.transform !== normal.transform || state.drop !== 'hidden')) {
    throw new Error('Reduced-motion render did not remain at the static original mark.');
  }
  await motionPreference('no-preference');
  const interaction = {};
  interaction.breathing = await evaluate('document.querySelector("[data-study=breathe]").click(); window.logoMotion.inspect()');
  interaction.paused = await evaluate('document.querySelector("#pause").click(); window.logoMotion.inspect()');
  await evaluate('document.querySelector("#stage").focus()');
  await call('Input.dispatchKeyEvent', { type: 'keyDown', code: 'Space', key: ' ', windowsVirtualKeyCode: 32 });
  interaction.keyDown = await evaluate('window.logoMotion.inspect()');
  await call('Input.dispatchKeyEvent', { type: 'keyUp', code: 'Space', key: ' ', windowsVirtualKeyCode: 32 });
  interaction.keyUp = await evaluate('window.logoMotion.inspect()');
  await evaluate('document.querySelector("#pause").click()');
  const center = await evaluate('(() => { const box = document.querySelector("#stage").getBoundingClientRect(); return {x: box.x + box.width / 2, y: box.y + box.height / 2}; })()');
  await call('Input.dispatchMouseEvent', { type: 'mousePressed', ...center, button: 'left', clickCount: 1 });
  interaction.pointerDown = await evaluate('window.logoMotion.inspect()');
  await call('Input.dispatchMouseEvent', { type: 'mouseReleased', ...center, button: 'left', clickCount: 1 });
  interaction.pointerUp = await evaluate('window.logoMotion.inspect()');
  await evaluate('document.querySelector("#pause").click(); document.querySelector("#theme").click()');
  await screenshot(path.join(output, 'light-theme.png'), 1280, 1000);
  await evaluate('document.querySelector("#theme").click()');
  await evaluate('window.logoMotion.seek("press", .18)');
  const compressed = await evaluate('window.logoMotion.inspect()');
  await evaluate('window.logoMotion.seek("press", .48)');
  const released = await evaluate('window.logoMotion.inspect()');
  await evaluate('window.logoMotion.seek("press", 2)');
  const settled = await evaluate('window.logoMotion.inspect()');
  const manifest = { fps: 30, frameWidth: 640, frameHeight: 480, perAtlas: 8, work, normal, reduced, interaction, compressed, released, settled, clips: [] };
  for (const [kind, duration] of [['open', 3], ['arrival', 3.2], ['breathe', 4], ['press', 3]]) {
    const count = Math.round(duration * manifest.fps);
    const files = [];
    for (let frame = 0; frame < count; frame += manifest.perAtlas) {
      const times = Array.from({ length: Math.min(manifest.perAtlas, count - frame) }, (_, index) => (frame + index) / manifest.fps);
      const dimensions = await evaluate(`window.logoMotion.captureFrames(${JSON.stringify(kind)}, ${JSON.stringify(times)})`);
      const file = path.join(work, `${kind}-${String(frame).padStart(4, '0')}.png`);
      await screenshot(file, dimensions.width, dimensions.height);
      files.push({ file, frames: times.length });
    }
    manifest.clips.push({ kind, duration, count, files });
    process.stdout.write(`Captured ${kind}: ${count} frames\n`);
  }
  await writeFile(path.join(output, 'capture-manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
  await evaluate('window.logoMotion.endCapture()');
} finally {
  if (socket.readyState === WebSocket.OPEN) await send('Browser.close').catch(() => {});
  socket.close();
  lines.close();
}
