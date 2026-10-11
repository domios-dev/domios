import { webkit, devices } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, writeFile } from 'node:fs/promises';
import { paintedGapSamples } from './chat-scroll-criteria.mjs';

const api = process.env.CODETERM_API_URL;
const token = process.env.CODETERM_API_TOKEN;
const tabId = process.env.CHAT_QA_TAB ?? '91a91705';
const out = process.env.CHAT_QA_OUT ?? 'target-check/chat-live-scroll';
if (!api || !token || new URL(api).port === '7685') throw new Error('Run through evals/_lib/lib.sh dev_run against a DEV instance');
const headers = { Authorization: `Bearer ${token}` };
const health = await fetch(`${api}/api/health`, { headers }).then((r) => r.json());
if (process.env.CHAT_QA_SHA && !health.build_sha?.startsWith(process.env.CHAT_QA_SHA)) {
  throw new Error(`Stale DEV build: ${health.build_sha}`);
}
await mkdir(out, { recursive: true });
const report = { health, tabId, cases: [], failures: [] };
const browser = await webkit.launch({ headless: true });
const runArrival = promisify(execFile);
const runId = Date.now().toString(36);

async function observe(page) {
  await page.evaluate(() => {
    const conversation = document.querySelector('[aria-label="Tab chat: Chat"] [aria-label="Conversation"]');
    if (!conversation) throw new Error('Conversation is absent');
    window.chatScrollQA?.dispose();
    const samples = [];
    let frame = null;
    const sample = (edge) => {
      const rows = Array.from(conversation.querySelectorAll('[data-message-id]'));
      const tail = rows.at(-1);
      const box = tail?.getBoundingClientRect();
      const viewport = conversation.getBoundingClientRect();
      samples.push({ edge, at: performance.now(), top: conversation.scrollTop,
        height: conversation.scrollHeight, client: conversation.clientHeight,
        gap: conversation.scrollHeight - conversation.clientHeight - conversation.scrollTop,
        tailId: tail?.dataset.messageId, tailText: tail?.textContent?.slice(-160),
        tailVisible: !!box && box.bottom <= viewport.bottom + 1 && box.bottom > viewport.top });
    };
    const changed = () => {
      if (frame !== null) return;
      frame = requestAnimationFrame(() => { frame = null; sample('paint'); });
    };
    const mutation = new MutationObserver(changed);
    mutation.observe(conversation, { subtree: true, childList: true, characterData: true, attributes: true });
    const resize = new ResizeObserver(changed);
    resize.observe(conversation);
    if (conversation.firstElementChild) resize.observe(conversation.firstElementChild);
    conversation.addEventListener('scroll', changed);
    window.chatScrollQA = { samples, sample, dispose: () => {
      mutation.disconnect(); resize.disconnect(); conversation.removeEventListener('scroll', changed);
      if (frame !== null) cancelAnimationFrame(frame);
    } };
    sample('installed');
  });
}

async function waitDOM(page, marker, committed = false, role = null, excluded = []) {
  await page.evaluate(({ marker, committed, role, excluded }) => new Promise((resolve, reject) => {
    const signal = AbortSignal.timeout(60000);
    const check = () => {
      const row = Array.from(document.querySelectorAll('[aria-label="Tab chat: Chat"] [data-message-id]')).find((node) =>
        node.textContent?.includes(marker) && (!role || node.dataset.messageRole === role)
        && !excluded.includes(node.dataset.messageId) && (!committed || !node.dataset.messageId.startsWith('opt-')));
      if (!row) return;
      cleanup(); resolve(row.dataset.messageId);
    };
    const observer = new MutationObserver(check);
    const fail = () => { cleanup(); reject(new Error(`Missing live DOM turn: ${marker}, committed=${committed}`)); };
    const cleanup = () => { observer.disconnect(); signal.removeEventListener('abort', fail); };
    observer.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true });
    signal.addEventListener('abort', fail, { once: true }); check();
  }), { marker, committed, role, excluded });
}

async function send(page, text, label) {
  await page.locator(`[data-testid="chat-composer"][data-tab-id="${tabId}"]`).fill(text);
  const installHint = page.getByTestId('pwa-install-hint-dismiss');
  if (await installHint.isVisible()) await installHint.click();
  await page.evaluate(() => window.chatScrollQA.sample('before-send'));
  await page.getByRole('region', { name: 'Tab chat: Chat', exact: true }).getByTestId('composer-send-button').click();
  await waitDOM(page, label);
  await page.evaluate(() => window.chatScrollQA.sample('optimistic'));
}

async function arrivals(page, viewport) {
  for (const kind of ['routed', 'delayed']) {
    for (const detached of [false, true]) {
      const label = `ARRIVAL-${runId}-${viewport.width}-${kind}-${detached}`;
      const result = { viewport, kind, detached, label, samples: [], errors: [] };
      report.cases.push(result);
      await page.locator('[aria-label="Chat tab status Idle"]').waitFor({ state: 'attached', timeout: 60000 });
      const priorAssistants = await page.getByRole('region', { name: 'Tab chat: Chat', exact: true })
        .locator('[data-message-role="assistant"]').evaluateAll((rows) => rows.map((row) => row.dataset.messageId));
      await send(page, `UI QA fixture: write 100 paragraphs about scrolling. Do not use tools or contact agents. SEED-${label}`, `SEED-${label}`);
      await waitDOM(page, '', false, 'assistant', priorAssistants);
      const status = await fetch(`${api}/api/tabs/${tabId}/status`, { headers }).then((r) => r.json());
      if (status.status !== 'working') throw new Error('Arrival precondition missing: agent is not streaming');
      const anchor = await page.evaluate((detached) => {
        const el = document.querySelector('[aria-label="Tab chat: Chat"] [aria-label="Conversation"]');
        if (detached) el.scrollTop = Math.max(0, el.scrollTop - 300);
        const row = Array.from(el.querySelectorAll('[data-message-id]')).find((node) =>
          node.getBoundingClientRect().bottom > el.getBoundingClientRect().top);
        return { id: row?.dataset.messageId, offset: row?.getBoundingClientRect().top - el.getBoundingClientRect().top };
      }, detached);
      await page.evaluate(() => { window.chatScrollQA.samples.length = 0; window.chatScrollQA.sample('before-arrival'); });
      try {
        await runArrival('bash', ['scripts/qa/chat-arrival.sh', tabId, label, kind]);
        if (detached) {
          await page.getByRole('button', { name: /Show \d+ new messages/ }).waitFor({ timeout: 60000 });
          result.anchorAfter = await page.evaluate((anchor) => {
            const el = document.querySelector('[aria-label="Tab chat: Chat"] [aria-label="Conversation"]');
            const row = Array.from(el.querySelectorAll('[data-message-id]')).find((node) => node.dataset.messageId === anchor.id);
            return { id: row?.dataset.messageId, offset: row?.getBoundingClientRect().top - el.getBoundingClientRect().top };
          }, anchor);
          if (result.anchorAfter.id !== anchor.id || Math.abs(result.anchorAfter.offset - anchor.offset) > 1) {
            result.errors.push('Detached content anchor moved on arrival');
          }
          await page.evaluate(() => window.chatScrollQA.sample('detached-arrival'));
          result.detachedSamples = await page.evaluate(() => window.chatScrollQA.samples);
          await page.getByRole('button', { name: /Show \d+ new messages/ }).click();
          await page.evaluate(() => { window.chatScrollQA.samples.length = 0; });
        }
        await waitDOM(page, label, true, 'user');
        await waitDOM(page, `ARRIVAL-DONE-${label}`, true, 'assistant');
        await page.evaluate(() => window.chatScrollQA.sample('arrival-reply'));
        result.samples = await page.evaluate(() => window.chatScrollQA.samples);
        if (result.samples.some((sample) => sample.gap > 1)) result.errors.push('Arrival moved the pinned viewport off bottom');
      } catch (error) {
        result.errors.push(error.message);
        result.transcript = await fetch(`${api}/api/tabs/${tabId}/chat`, { headers }).then((r) => r.json());
        await page.screenshot({ path: `${out}/${label}.png` });
      }
      await writeFile(`${out}/report.json`, JSON.stringify(report, null, 2));
      if (result.errors.length) { report.failures.push(label); throw new Error('Incoming-turn scroll case failed'); }
    }
  }
}

try {
  for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]
    .filter((size) => !process.env.CHAT_QA_WIDTH || size.width === Number(process.env.CHAT_QA_WIDTH))) {
    const context = await browser.newContext({ ...devices['iPhone 15'], viewport, screen: viewport });
    const page = await context.newPage();
    console.log('Opening', viewport);
    await page.goto(`${api}/#token=${encodeURIComponent(token)}`);
    const openSidebar = page.getByTestId('web-sidebar-open');
    if (!(await page.locator('button[aria-label="Select workspace MobileQA"]').isVisible())
      && await openSidebar.isVisible()) await openSidebar.click();
    await page.locator('button[aria-label="Select workspace MobileQA"]').click();
    if (!(await page.getByText('1 utility shell', { exact: true }).isVisible())
      && !(await page.getByRole('button', { name: 'Open tab Chat', exact: true }).isVisible())
      && await openSidebar.isVisible()) await openSidebar.click();
    await writeFile(`${out}/navigation.txt`, await page.locator('body').innerText());
    const shells = page.getByText('1 utility shell', { exact: true });
    if (await shells.isVisible()) await shells.click();
    await page.getByRole('button', { name: 'Open tab Chat', exact: true }).click();
    await page.locator(`[data-testid="chat-composer"][data-tab-id="${tabId}"]`).waitFor();
    const latest = page.getByRole('region', { name: 'Tab chat: Chat', exact: true }).getByTestId('chat-scroll-bottom');
    if (await latest.isVisible()) await latest.click();
    console.log('Chat selected');
    await observe(page);
    console.log('Observer installed');
    for (const phase of ['idle', 'streaming']) {
      for (let index = 0; index < 10; index += 1) {
        const label = `SCROLL-${runId}-${viewport.width}-${phase}-${index}`;
        const done = `DONE-${label}`;
        const result = { viewport, phase, index, label, samples: [], errors: [] };
        report.cases.push(result);
        await page.evaluate(() => { window.chatScrollQA.samples.length = 0; });
        try {
          await page.locator('[aria-label="Chat tab status Idle"]').waitFor({ state: 'attached', timeout: 60000 });
          const priorAssistants = await page.getByRole('region', { name: 'Tab chat: Chat', exact: true })
            .locator('[data-message-role="assistant"]').evaluateAll((rows) => rows.map((row) => row.dataset.messageId));
          if (index === 0 && phase === 'idle') {
            await page.evaluate(() => {
              const el = document.querySelector('[aria-label="Tab chat: Chat"] [aria-label="Conversation"]');
              el.scrollTop = Math.max(0, el.scrollTop - 300);
            });
          }
          console.log('Sending', label);
          await send(page, phase === 'idle'
            ? `Reply with exactly ${done}. ${label}`
            : `Write 80 numbered paragraphs about scrolling and finish with ${done}. ${label}`, label);
          if (phase === 'streaming') {
            await waitDOM(page, '', false, 'assistant', priorAssistants);
            const status = await fetch(`${api}/api/tabs/${tabId}/status`, { headers }).then((r) => r.json());
            result.statusAfterSend = status;
            if (status.status !== 'working') throw new Error('Streaming precondition missing: agent is not working');
            const steer = `STEER-${label}`;
            await send(page, `While working, include ${steer} in your final answer.`, steer);
            await waitDOM(page, steer, true);
          }
          await waitDOM(page, label, true);
          await page.evaluate(() => window.chatScrollQA.sample('echo'));
          await waitDOM(page, done, true, 'assistant');
          await page.evaluate(() => window.chatScrollQA.sample('reply'));
        } catch (error) {
          result.errors.push(error.message);
          result.transcript = await fetch(`${api}/api/tabs/${tabId}/chat`, { headers }).then((r) => r.json());
          await page.screenshot({ path: `${out}/${label}.png` });
        }
        result.samples = await page.evaluate(() => window.chatScrollQA.samples);
        const jumps = paintedGapSamples(result.samples, { afterSend: true });
        if (jumps.length) result.errors.push(`${jumps.length} painted samples outside bottom`);
        console.log(JSON.stringify({ label, errors: result.errors, samples: result.samples.length }));
        if (result.errors.length) report.failures.push(label);
        await writeFile(`${out}/report.json`, JSON.stringify(report, null, 2));
        if (result.errors.length) {
          throw new Error('Live transcript stalled; stopping instead of sending into a blind view');
        }
      }
    }
    await arrivals(page, viewport);
    await page.reload();
    await page.locator(`[data-testid="chat-composer"][data-tab-id="${tabId}"]`).waitFor();
    await observe(page);
    await page.evaluate(() => window.chatScrollQA.sample('reload'));
    report.reloadSamples = await page.evaluate(() => window.chatScrollQA.samples);
    if (report.reloadSamples.some((sample) => sample.gap > 1)) throw new Error('Reload moved the pinned viewport off bottom');
    await context.close();
  }
} catch (error) {
  report.error = error.message;
  report.failures.push(error.message);
} finally {
  await writeFile(`${out}/report.json`, JSON.stringify(report, null, 2));
  await browser.close();
}
console.log(`Evidence: ${out}/report.json; failures=${report.failures.length}`);
process.exitCode = report.failures.length ? 1 : 0;
