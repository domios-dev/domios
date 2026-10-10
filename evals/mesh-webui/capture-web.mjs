import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { validateOutput } from './model.mjs';
import { createHarness, switchPeer, capture, selectTab, terminalObserver, deadline } from './browser.mjs';

if (process.env.B3_GO_LIVE !== 'yes' || process.env.B2_GO_LIVE !== 'yes') {
  throw new Error('Requires explicit Orchestrator GO live for shared B2/B3 captures');
}
const [manifestFile, runDirectory, casesFile] = process.argv.slice(2);
if (!manifestFile || !runDirectory || !casesFile) throw new Error('Usage: capture-web.mjs MANIFEST RC_RUN CASES');
const out = validateOutput(runDirectory);
if (!path.basename(out).startsWith('mesh-terminals-')) throw new Error('B3 requires a mesh-terminals run');
const manifest = JSON.parse(await readFile(manifestFile, 'utf8'));
const cases = JSON.parse(await readFile(casesFile, 'utf8'));
if (!Array.isArray(cases) || !cases.length) throw new Error('At least one capture case required');
const ids = new Set();
for (const item of cases) {
  if (!/^[a-zA-Z0-9_-]+$/.test(item.id) || ids.has(item.id)) throw new Error('Unique safe capture id required');
  ids.add(item.id);
  if (!['mac', 'windows'].includes(item.gateway) || !['mac', 'windows'].includes(item.home)) throw new Error('Unknown machine');
  if (!['local', 'remote', 'web'].includes(item.view)) throw new Error('Unknown view');
  if (item.view !== 'web') throw new Error('This entry captures web only; desktop capture is separate evidence');
  const fixtures = [...manifest[item.home].tabs, ...(manifest[item.home].shells ?? [])];
  if (!fixtures.some(tab => tab.tabId === item.tabId)) throw new Error('Capture target must be a manifest-owned fixture');
}
const harness = await createHarness(manifest, out);
try {
  for (const item of cases) {
    const { page, diagnostics } = await harness.openGateway(item.gateway, { diagnosticsDirectory: path.join(out, item.id) });
    let complete = false;
    try {
      const intendedPeer = item.gateway === item.home ? null : manifest[item.home].peerId;
      const current = await page.evaluate(() => localStorage.getItem(`codeterm-web-relay-peer:${location.origin}`));
      if ((current || null) !== intendedPeer) await switchPeer(page, intendedPeer);
      await selectTab(page, item.tabId, { mode: 'terminal' });
      const base = await capture(page, item.id + '-provenance', { tabId: item.tabId });
      const probeId = await terminalObserver.armTerminalEvents(page, item.tabId);
      try {
        await terminalObserver.captureTerminal(page, {
          tabId: item.tabId, probeId, directory: path.join(out, item.id),
          provenance: { ...base.metadata, home: item.home, view: 'web', fixtureTabId: item.tabId },
        });
      } catch (error) {
        await writeFile(path.join(out, item.id + '-capture-failure.json'), JSON.stringify({
          id: item.id, tabId: item.tabId, home: item.home, gateway: item.gateway,
          error: error.message, verdict: 'capture failure; product verdict requires step repro',
        }, null, 2), { flag: 'wx' });
        throw error;
      } finally {
        await terminalObserver.releaseTerminalEvents(page, probeId);
      }
      if (intendedPeer !== null) await switchPeer(page, null);
      await capture(page, item.id + '-gateway-return');
      complete = true;
    } finally {
      try { await diagnostics.finish(complete ? 'case-complete' : 'case-failed'); }
      finally { await deadline(page.context().close(), 'B3 context close'); }
    }
  }
} finally {
  await harness.close();
}
