import { createHash } from 'node:crypto';
import { once } from 'node:events';
import { createRequire } from 'node:module';
import { access, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createInterface } from 'node:readline';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const directory = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const typescript = require('typescript');
const options = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  const key = process.argv[index];
  if (!key.startsWith('--') || options.has(key) || !process.argv[index + 1]) throw new Error(`Invalid argument: ${key}`);
  options.set(key, process.argv[index + 1]);
}

const output = options.get('--output-dir');
const fps = Number(options.get('--fps') ?? 30);
const duration = Number(options.get('--duration') ?? 1.6);
if (!output || fps !== 30 || duration !== 1.6 || options.size !== 3
  || ['--output-dir', '--fps', '--duration'].some(key => !options.has(key))) {
  throw new Error('Usage: node media-src/export-threshold.mjs --output-dir PATH --fps 30 --duration 1.6');
}

const outputDirectory = path.resolve(output);
await mkdir(outputDirectory, { recursive: true });
if ((await readdir(outputDirectory)).length) throw new Error(`Output directory must be empty: ${outputDirectory}`);

const sourcePath = path.resolve(directory, '../src/brand/motion.ts');
const source = await readFile(sourcePath, 'utf8');
const sourceSha256 = createHash('sha256').update(source).digest('hex');
const compiled = typescript.transpile(source, {
  target: typescript.ScriptTarget.ES2020,
  module: typescript.ModuleKind.CommonJS,
});
const motionModule = { exports: {} };
new Function('module', 'exports', compiled)(motionModule, motionModule.exports);
const { REST, surfaceTension } = motionModule.exports;
if (typeof surfaceTension !== 'function' || !REST) throw new Error('Authoritative motion exports are unavailable.');

const poses = Array.from({ length: Math.round(fps * duration) + 1 }, (_, frame) => {
  const time = frame === fps * duration ? duration : frame / fps;
  const pose = surfaceTension(time);
  return { frame, time, ...pose };
});
const last = poses.at(-1);
if (last.path !== REST.path || last.scaleX !== REST.scaleX || last.scaleY !== REST.scaleY || last.y !== REST.y) {
  throw new Error('Final exported pose does not equal authoritative REST.');
}

function svg(pose) {
  const transform = `translate(32 ${40 + pose.y}) scale(${pose.scaleX} ${pose.scaleY}) translate(-32 -40)`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="-4 -4 72 72"><g transform="${transform}"><path fill="#58a6ff" fill-rule="evenodd" d="${pose.path}"/></g></svg>`;
}

const work = await mkdtemp(path.join(os.tmpdir(), 'domios-threshold-'));
const browserPath = process.env.DOMIOS_EDGE_PATH ?? 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const browser = spawn(browserPath, [
  '--headless', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
  '--remote-debugging-port=0', `--user-data-dir=${path.join(work, 'profile')}`, 'about:blank',
], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
const lines = createInterface({ input: browser.stderr });
let socket;
let spawned = false;
let spawnFailed = false;
let closeBrowser = async () => {};
browser.once('spawn', () => { spawned = true; });
browser.on('error', () => { if (!spawned) spawnFailed = true; });

try {
  const endpoint = await new Promise((resolve, reject) => {
    const deadline = AbortSignal.timeout(60000);
    deadline.addEventListener('abort', () => reject(new Error('Edge DevTools endpoint deadline exceeded.')), { once: true });
    lines.on('line', line => {
      const match = line.match(/DevTools listening on (ws:\/\/\S+)/);
      if (match) resolve(match[1]);
    });
    browser.once('error', reject);
    browser.once('exit', code => reject(new Error(`Edge exited before DevTools started: ${code}`)));
  });

  socket = new WebSocket(endpoint);
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
    for (const recipient of pending.values()) recipient.reject(new Error('Edge connection closed.'));
    pending.clear();
  });

  const send = (method, params = {}, sessionId, timeoutMs = 60000) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const deadline = AbortSignal.timeout(timeoutMs);
    deadline.addEventListener('abort', () => {
      if (pending.delete(id)) reject(new Error(`${method} deadline exceeded.`));
    }, { once: true });
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
  closeBrowser = () => send('Browser.close', {}, undefined, 5000);
  const event = (method, sessionId) => new Promise((resolve, reject) => {
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

  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const call = (method, params) => send(method, params, sessionId);
  const evaluate = async expression => {
    const result = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  await call('Page.enable', {});
  await call('Emulation.setDeviceMetricsOverride', { width: 256, height: 256, deviceScaleFactor: 1, mobile: false });
  await call('Emulation.setDefaultBackgroundColorOverride', { color: { r: 0, g: 0, b: 0, a: 0 } });
  const loaded = event('Page.loadEventFired', sessionId);
  await call('Page.navigate', { url: 'data:text/html;charset=utf-8,%3C!doctype%20html%3E%3Cbody%3E%3C%2Fbody%3E' });
  await loaded;

  for (const pose of poses) {
    const markup = svg(pose);
    await evaluate(`document.body.style.margin='0';document.body.innerHTML=${JSON.stringify(markup)};document.querySelector('svg').getBBox().width`);
    await evaluate('new Promise(requestAnimationFrame)');
    const screenshot = await call('Page.captureScreenshot', {
      format: 'png', captureBeyondViewport: true,
      clip: { x: 0, y: 0, width: 256, height: 256, scale: 1 },
    });
    const name = `frame-${String(pose.frame).padStart(4, '0')}.png`;
    const png = Buffer.from(screenshot.data, 'base64');
    await writeFile(path.join(outputDirectory, name), png);
    pose.png = name;
    pose.pngSha256 = createHash('sha256').update(png).digest('hex');
    pose.svgSha256 = createHash('sha256').update(markup).digest('hex');
  }

  const manifest = {
    schemaVersion: 1,
    fps,
    duration,
    width: 256,
    height: 256,
    restFrame: poses.length - 1,
    source: { path: sourcePath, sha256: sourceSha256 },
    poses,
  };
  await writeFile(path.join(outputDirectory, 'brand-frames.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  process.stdout.write(`Exported ${poses.length} authoritative Threshold poses (${sourceSha256}).\n`);
} finally {
  lines.close();
  const tempRoot = path.resolve(os.tmpdir());
  const workPath = path.resolve(work);
  const relativeWork = path.relative(tempRoot, workPath);
  if (!relativeWork || path.isAbsolute(relativeWork) || relativeWork === '..'
    || relativeWork.startsWith(`..${path.sep}`) || relativeWork.includes(path.sep)
    || !path.basename(workPath).startsWith('domios-threshold-')) {
    throw new Error(`Refusing to remove a work directory outside the generated temp scope: ${workPath}`);
  }
  if (socket?.readyState === WebSocket.OPEN) {
    await closeBrowser().catch(() => {});
    socket.close();
  }
  const waitForExit = async timeoutMs => {
    if (spawnFailed || browser.exitCode !== null || browser.signalCode !== null) return true;
    try {
      await once(browser, 'exit', { signal: AbortSignal.timeout(timeoutMs) });
      return true;
    } catch (error) {
      if (error.name !== 'AbortError') throw error;
      return false;
    }
  };
  let exited = await waitForExit(10000);
  if (!exited) {
    browser.kill();
    exited = await waitForExit(10000);
  }
  if (!exited) throw new Error(`Edge did not exit; retaining its generated temp profile at ${workPath}`);
  await rm(workPath, { recursive: true });
  try {
    await access(workPath);
    throw new Error(`Generated temp profile remains after cleanup: ${workPath}`);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
}
