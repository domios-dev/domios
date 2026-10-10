import {createRequire} from 'node:module';
import {readFile, mkdir, writeFile} from 'node:fs/promises';
import path from 'node:path';
import {validateManifest,validateOutput} from './model.mjs';
import {gatewayToken} from './secrets.mjs';
import * as rawTerminal from './terminal-capture.mjs';
import {readSelectedTab} from './provenance.mjs';
import {activateAndSelectFixture,fixtureWorkspaceId} from './workspace-selection.mjs';
const rc = '/Users/dev/Developer/codeterm-worktrees/rc-mac';
const require = createRequire(path.join(rc, 'package.json'));
export const STEP_MS = 30_000;
export async function deadline(promise, label, ms = STEP_MS) {
  const signal = AbortSignal.timeout(ms);
  let fail;
  const timeout = new Promise((_, reject) => {
    fail = () => reject(new Error(`${label}: bounded failure deadline ${ms}ms`));
    signal.addEventListener('abort', fail, {once:true});
  });
  try { return await Promise.race([promise, timeout]); }
  finally { signal.removeEventListener('abort', fail); }
}
export async function waitDom(page, spec) {
  return deadline(page.evaluate(spec => new Promise(resolve => {
    let observer;
    const read = () => {
      const nodes = [...document.querySelectorAll(spec.selector)].filter(e => e.getClientRects().length);
      const element = nodes.find(e => (!spec.attr || e.getAttribute(spec.attr) === spec.value)
        && (!spec.text || e.textContent.includes(spec.text)));
      if (element || (spec.absent && !nodes.length)) {
        observer?.disconnect(); resolve(element?.outerHTML ?? 'absent'); return true;
      }
      return false;
    };
    observer = new MutationObserver(read);
    observer.observe(document.documentElement, {subtree:true,childList:true,attributes:true,characterData:true});
    read();
  }), spec), `DOM ${spec.selector}`);
}
export async function switchPeer(page, peerIdOrNull) {
  await page.getByTestId('peer-picker-button').click();
  await page.evaluate(() => {
    window.__b2Switch = new Promise(resolve => {
      let switchId;
      const reset = event => { switchId = event.detail.switchId; };
      const ready = event => {
        if (event.detail.switchId !== switchId) return;
        window.removeEventListener('codeterm-peer-switch-reset', reset);
        window.removeEventListener('codeterm-peer-switch-ready', ready);
        resolve(event.detail);
      };
      window.addEventListener('codeterm-peer-switch-reset', reset);
      window.addEventListener('codeterm-peer-switch-ready', ready);
    });
  });
  if (peerIdOrNull) await page.locator(`[data-testid="peer-picker-item"][data-peer-id="${peerIdOrNull}"]`).click();
  else await page.getByTestId('peer-picker-self').click();
  const receipt = await deadline(page.evaluate(() => window.__b2Switch), 'Peer switch acknowledgement');
  if (!receipt.success) throw new Error('Peer switch ready reported success=false');
  const current = await page.evaluate(() => localStorage.getItem(`codeterm-web-relay-peer:${location.origin}`));
  if ((current || null) !== peerIdOrNull) throw new Error('Selected peer differs from intended target');
  delete page.__b2SelectedTabId;
  return receipt;
}
export async function selectTab(page, tabId, {mode = 'chat', workspaceId} = {}) {
  if (!workspaceId) {
    const peerId = await page.evaluate(() => localStorage.getItem(`codeterm-web-relay-peer:${location.origin}`));
    workspaceId = fixtureWorkspaceId(page.__b2Manifest, page.__b2Gateway, peerId, tabId);
  }
  const tab = await activateAndSelectFixture(page, {workspaceId,tabId}, waitDom);
  await waitDom(page, {selector:`[data-testid="terminal-tab"][data-tab-id="${tabId}"]`});
  const toggle = page.locator(`[data-testid="tab-mode-switch"][data-tab-id="${tabId}"] [data-value="${mode}"]`);
  if (await toggle.count()) await toggle.click();
  const selector = mode === 'terminal'
    ? `[data-testid="terminal-tab"][data-tab-id="${tabId}"] [data-testid="terminal-frame"][data-terminal-visible="true"]`
    : `[data-testid="tab-session-chat"][data-tab-id="${tabId}"]`;
  await waitDom(page, {selector});
  page.__b2SelectedTabId = tabId;
  return tab;
}
export const terminalObserver = Object.fromEntries(Object.entries(rawTerminal).map(([key, fn]) => [key,
  key === 'readTerminalState' ? fn : (...args) => deadline(fn(...args), `B3 ${key}`)]));
export async function captureTerminalView(page, tabId, label) {
  await selectTab(page, tabId, {mode:'terminal'});
  const provenance = (await capture(page, label+'-provenance', {tabId})).metadata;
  const probeId = await terminalObserver.armTerminalEvents(page, tabId);
  try {
    return await terminalObserver.captureTerminal(page, {tabId,probeId,
      directory:path.join(page.__b2Out,label),provenance});
  } finally { await terminalObserver.releaseTerminalEvents(page,probeId); }
}
export async function observe(page) {
  await page.evaluate(() => {
    window.__b2Activity?.observer.disconnect();
    const events = [];
    let prior = '';
    const collect = () => {
      const visible = s => [...document.querySelectorAll(s)].find(e => e.getClientRects().length);
      const chat = visible('[data-testid="tab-session-chat"]');
      const agent = chat?.querySelector('[data-testid="agent-state"]');
      const log = chat?.querySelector('[role="log"]');
      const row = {tabId:chat?.getAttribute('data-tab-id'), state:agent?.getAttribute('data-agent-state'),
        phase:agent?.getAttribute('data-agent-phase'), status:agent?.textContent,
        sending:!!chat?.querySelector('[data-state="sending"]'),
        progress:log?.textContent?.slice(-4000), elapsed:agent?.textContent?.match(/\b\d+(?:\.\d+)?\s*(?:s|sec|seconds|m|min)\b/)?.[0] ?? null};
      const serialized = JSON.stringify(row);
      if (serialized !== prior) { events.push({...row,at:Date.now(),monotonic:performance.now()}); prior=serialized; }
    };
    const observer = new MutationObserver(collect);
    observer.observe(document.documentElement, {subtree:true,childList:true,attributes:true,characterData:true});
    window.__b2Activity = {observer,events}; collect();
  });
}
export async function activity(page) { return page.evaluate(() => window.__b2Activity?.events ?? []); }
export async function capture(page, label, {tabId = page.__b2SelectedTabId} = {}) {
  if (!page.__b2Out) throw new Error('Page must come from shared harness');
  const name = label.replace(/[^a-zA-Z0-9_.-]/g,'_');
  const selected = await page.evaluate(readSelectedTab, {tabId});
  const metadata = {...selected, ...await page.evaluate(() => ({at:Date.now(),origin:location.origin,
    peerId:localStorage.getItem(`codeterm-web-relay-peer:${location.origin}`),
    tabIds:[...document.querySelectorAll('[data-testid="tab-item"]')].map(e => e.getAttribute('data-tab-id')),
    bodyText:document.body.innerText, activities:window.__b2Activity?.events ?? []}))};
  await page.screenshot({path:path.join(page.__b2Out, name+'.png'),fullPage:true,timeout:STEP_MS});
  await writeFile(path.join(page.__b2Out, name+'.json'), JSON.stringify(metadata,null,2));
  return {evidence:name+'.json',screenshot:name+'.png',metadata};
}
export async function createHarness(manifest, outDir) {
  if (process.env.B2_GO_LIVE !== 'yes') throw new Error('Requires explicit Orchestrator GO live');
  validateManifest(manifest);
  validateOutput(outDir);
  await mkdir(outDir,{recursive:true});
  const {chromium} = require('@playwright/test');
  const browser = await chromium.launch({headless:true,timeout:STEP_MS});
  const contexts = [];
  return {browser, async openGateway(machineKey) {
    const machine = manifest[machineKey];
    if (!machine || !['mac','windows'].includes(machineKey)) throw new Error('Unknown gateway');
    const token = await gatewayToken(machine);
    if (!token) throw new Error('Missing gateway token');
    const context = await browser.newContext({viewport:{width:1440,height:1000}});
    contexts.push(context);
    context.setDefaultTimeout(STEP_MS);
    context.setDefaultNavigationTimeout(STEP_MS);
    await context.route('**/*', route => {
      const u = new URL(route.request().url());
      return u.origin === machine.origin || ['data:','blob:'].includes(u.protocol) ? route.continue() : route.abort('blockedbyclient');
    });
    await context.addInitScript(({origin,token}) => {
      if (location.origin === origin) localStorage.setItem('codeterm-web-token:'+origin,token);
    }, {origin:machine.origin,token});
    const page = await context.newPage();
    const evaluate = page.evaluate.bind(page);
    page.evaluate = (...args) => deadline(evaluate(...args), 'Browser protocol evaluate');
    page.__b2Out = outDir;
    page.__b2Manifest = manifest;
    page.__b2Gateway = machineKey;
    await page.goto(machine.origin, {waitUntil:'domcontentloaded'});
    await page.getByTestId('peer-picker-button').waitFor({state:'visible'});
    return {page,context,provenance:{gateway:machineKey,origin:machine.origin}};
  }, async close() {
    for (const context of contexts) await deadline(context.close(),'Context close');
    await deadline(browser.close(),'Browser close');
  }};
}
