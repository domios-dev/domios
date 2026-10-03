import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { openBrowser } from './cdp.mjs';

const outputDir = process.argv[2];
if (!outputDir) throw new Error('Usage: node tools/record.mjs OUTPUT_DIRECTORY');
const base = process.env.MOTION_LAB_URL ?? 'http://127.0.0.1:5184';
const fps = 30;
const browser = await openBrowser(outputDir);
const results = [];
const click = selector => browser.evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);
const pointer = async (selector, type = 'mouseMoved') => {
  const position = await browser.evaluate(`(() => {const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  await browser.command('Input.dispatchMouseEvent', { type, ...position, button: type === 'mouseMoved' ? 'none' : 'left', clickCount: type === 'mouseMoved' ? 0 : 1 });
};
const studies = [
  { piece: 'mesh', seconds: 14, actions: [[11.8, () => click('.theme-button')]] },
  { piece: 'waterline', seconds: 6, actions: [[3, async () => { await click('.theme-button'); await click('.replay-study'); }]] },
  { piece: 'showcase', seconds: 9, actions: [[1.0, () => click('.caption-controls button:nth-child(2)')], [2.0, () => click('[role="tab"]:nth-of-type(2)')], [3.2, () => click('.caption-controls button:nth-child(3)')], [4.4, () => click('[role="tab"]:nth-of-type(3)')], [5.5, () => click('.theme-button')], [6.2, () => click('[role="tab"]:nth-of-type(1)')], [7.4, () => click('.caption-controls button:nth-child(2)')]] },
  { piece: 'gather', seconds: 8, actions: [[.8, () => pointer('.gather-button')], [1.7, () => pointer('.gather-button', 'mousePressed')], [1.95, () => pointer('.gather-button', 'mouseReleased')], [3.1, () => browser.command('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 600, y: 240 })], [4, () => click('.theme-button')], [4.7, () => pointer('.gather-button')], [5.6, () => pointer('.gather-button', 'mousePressed')], [5.85, () => pointer('.gather-button', 'mouseReleased')], [7, () => browser.command('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 600, y: 240 })]] },
];

try {
  await browser.command('Emulation.setDeviceMetricsOverride', { width: 1280, height: 1000, deviceScaleFactor: 1, mobile: false });
  for (const study of studies) {
    await browser.navigate(`${base}/?piece=${study.piece}&theme=light`);
    await browser.evaluate('window.recordedAnimations = new Map();');
    const path = join(outputDir, `${study.piece}-preview.mp4`);
    const encoder = spawn('ffmpeg', ['-y', '-hide_banner', '-loglevel', 'error', '-f', 'image2pipe', '-vcodec', 'mjpeg', '-framerate', String(fps), '-i', 'pipe:0', '-an', '-c:v', 'libx264', '-preset', 'fast', '-crf', '25', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', path], { windowsHide: true, stdio: ['pipe', 'ignore', 'pipe'] });
    let encodingError = '';
    encoder.stderr.on('data', chunk => { encodingError += chunk; });
    const completed = once(encoder, 'close');
    let actionIndex = 0;
    for (let frame = 0; frame < study.seconds * fps; frame++) {
      const time = frame / fps;
      while (study.actions[actionIndex]?.[0] <= time) {
        await study.actions[actionIndex][1]();
        actionIndex++;
      }
      await browser.evaluate(`(async () => {
        document.querySelector('.inhabited-mesh svg')?.dispatchEvent(new CustomEvent('motion-seek', {detail: ${time}}));
        await new Promise(resolve => requestAnimationFrame(resolve));
        for (const animation of document.getAnimations()) {
          if (!window.recordedAnimations.has(animation)) window.recordedAnimations.set(animation, ${time * 1000});
          animation.pause();
          animation.currentTime = ${time * 1000} - window.recordedAnimations.get(animation);
        }
      })()`);
      const { data } = await browser.command('Page.captureScreenshot', { format: 'jpeg', quality: 86, captureBeyondViewport: false });
      if (!encoder.stdin.write(Buffer.from(data, 'base64'))) await once(encoder.stdin, 'drain');
    }
    encoder.stdin.end();
    const [code] = await completed;
    if (code !== 0) throw new Error(`ffmpeg exit ${code}: ${encodingError}`);
    const { size } = await stat(path);
    if (size >= 3_000_000) throw new Error(`Preview exceeded 3MB: ${path} (${size})`);
    results.push({ piece: study.piece, path, bytes: size, seconds: study.seconds, fps, width: 1280, height: 1000, themes: ['light', 'dark'] });
    console.log(`Recorded ${study.piece}: ${(size / 1000).toFixed(0)} KB, ${study.seconds}s`);
  }
  await writeFile(join(outputDir, 'recordings.json'), JSON.stringify(results, null, 2));
} finally { await browser.close(); }
