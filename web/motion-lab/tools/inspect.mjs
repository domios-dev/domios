import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { openBrowser } from './cdp.mjs';

const outputDir = process.argv[2];
if (!outputDir) throw new Error('Usage: node tools/inspect.mjs OUTPUT_DIRECTORY');
const base = process.env.MOTION_LAB_URL ?? 'http://127.0.0.1:5184';
const browser = await openBrowser(outputDir);
const errors = [];
browser.on('Runtime.exceptionThrown', error => errors.push(error));
const results = [];
try {
  for (const theme of ['light', 'dark']) {
    for (const width of [1360, 390]) {
      await browser.command('Emulation.setDeviceMetricsOverride', { width, height: width === 390 ? 1100 : 850, deviceScaleFactor: 1, mobile: width === 390 });
      for (const piece of ['mesh', 'waterline', 'showcase', 'gather']) {
        await browser.navigate(`${base}/?piece=${piece}&theme=${theme}`);
        await browser.evaluate(`document.querySelector('.inhabited-mesh svg')?.dispatchEvent(new CustomEvent('motion-seek', {detail: 2.1})); document.getAnimations().forEach(animation => { animation.pause(); animation.currentTime = 1300; });`);
        const metrics = await browser.evaluate(`({width: innerWidth, scrollWidth: document.documentElement.scrollWidth, imageFailures: [...document.images].filter(image => !image.complete || !image.naturalWidth).map(image => image.src), buttons: [...document.querySelectorAll('button')].every(button => button.textContent.trim() || button.getAttribute('aria-label'))})`);
        if (metrics.scrollWidth > width || metrics.imageFailures.length || !metrics.buttons) throw new Error(JSON.stringify({ piece, width, theme, metrics }));
        const { data } = await browser.command('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true });
        await writeFile(join(outputDir, `${piece}-${theme}-${width}.png`), Buffer.from(data, 'base64'));
        results.push({ piece, width, theme, ...metrics });
      }
    }
  }
  await browser.command('Emulation.setDeviceMetricsOverride', { width: 1360, height: 850, deviceScaleFactor: 1, mobile: false });
  await browser.navigate(`${base}/?piece=showcase&theme=light`);
  await browser.evaluate(`document.querySelector('[role="tab"]').focus()`);
  for (const [key, code, expected] of [['ArrowRight', 'ArrowRight', 'A mesh'], ['End', 'End', 'Your tools'], ['Home', 'Home', 'A team']]) {
    await browser.command('Input.dispatchKeyEvent', { type: 'keyDown', key, code });
    await browser.command('Input.dispatchKeyEvent', { type: 'keyUp', key, code });
    const tab = await browser.evaluate(`({selected: document.querySelector('[role="tab"][aria-selected="true"]').textContent, focused: document.activeElement.getAttribute('aria-selected'), panelCount: [...document.querySelectorAll('[role="tabpanel"]')].filter(panel => !panel.hidden).length})`);
    if (!tab.selected.includes(expected) || tab.focused !== 'true' || tab.panelCount !== 1) throw new Error(JSON.stringify(tab));
    results.push({ keyboard: code, ...tab });
  }
  await browser.evaluate(`document.querySelector('.pause-study').click()`);
  await browser.evaluate(`document.querySelectorAll('[role="tab"]')[1].click(); document.querySelectorAll('.caption-controls button')[1].click()`);
  const pausedCaption = await browser.evaluate(`({opacity: getComputedStyle(document.querySelector('.liquid-caption')).opacity, panelOpacity: getComputedStyle(document.querySelector('.showcase-sheet')).opacity, caption: document.querySelector('.liquid-caption').textContent})`);
  if (pausedCaption.opacity !== '1' || pausedCaption.panelOpacity !== '1' || !pausedCaption.caption.includes('second home')) throw new Error(JSON.stringify(pausedCaption));
  results.push({ pausedCaption });
  await browser.navigate(`${base}/?piece=mesh&theme=light`);
  const frameSample = await browser.evaluate(`new Promise(resolve => {
    const times = [];
    const observe = now => {
      times.push(now);
      if (times.length < 181) requestAnimationFrame(observe);
      else {
        const intervals = times.slice(1).map((time, index) => time - times[index]).sort((a,b) => a-b);
        resolve({frames: intervals.length, medianMs: intervals[90], p95Ms: intervals[171], maxMs: intervals[179], over25Ms: intervals.filter(value => value > 25).length});
      }
    };
    requestAnimationFrame(observe);
  })`);
  results.push({ animationFrameSample: frameSample });
  await browser.evaluate(`document.querySelector('.pause-study').click()`);
  const stopped = await browser.evaluate(`Array.from(document.querySelectorAll('[data-parcel]'), node => node.getAttribute('transform'))`);
  await browser.evaluate(`new Promise(resolve => { let count = 0; const frame = () => ++count === 30 ? resolve() : requestAnimationFrame(frame); requestAnimationFrame(frame); })`);
  const held = await browser.evaluate(`Array.from(document.querySelectorAll('[data-parcel]'), node => node.getAttribute('transform'))`);
  if (JSON.stringify(stopped) !== JSON.stringify(held)) throw new Error('Hero moved while paused');
  results.push({ heroPauseHeldAcrossFrames: 30 });
  await browser.command('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
  for (const piece of ['mesh', 'waterline', 'showcase', 'gather']) {
    await browser.navigate(`${base}/?piece=${piece}&theme=dark`);
    const staticState = await browser.evaluate(`({motion: document.querySelector('.motion-lab').dataset.motion, animations: document.getAnimations().length, ledger: document.querySelector('.mesh-ledger')?.textContent, disabled: document.querySelector('.pause-study').disabled})`);
    if (staticState.motion !== 'reduced' || staticState.animations || !staticState.disabled || (piece === 'mesh' && !staticState.ledger.includes('comes home'))) throw new Error(JSON.stringify(staticState));
    results.push({ reducedMotion: piece, ...staticState });
  }
  if (errors.length) throw new Error(JSON.stringify(errors));
  await writeFile(join(outputDir, 'browser-checks.json'), JSON.stringify({ results, errors }, null, 2));
  console.log(`PASS: ${results.length} layout, keyboard, and reduced-motion observations. Screenshots: ${outputDir}`);
} finally { await browser.close(); }
