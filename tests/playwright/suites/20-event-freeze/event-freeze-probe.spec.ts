/**
 * WHAT: Quarantined desktop event-delivery and recovery probes for task_uwkt7p.
 * WHY: A stalled event stream previously froze sidebar state.
 * ORACLE: Sidebar projection and frontend/backend counters reconcile after faults.
 */
import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import { connectUi, API_BASE } from "../../helpers/dev-console";
import { readDevToken } from "../../helpers/api-client";

const TOKEN = readDevToken();
const JSON_H = { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" };
const AUTH_H = { Authorization: `Bearer ${TOKEN}` };
const CYCLES = Number(process.env.SOAK_CYCLES ?? 500);
const PROBE_EVERY = 50;

interface NewTab {
  tabId: string;
  tabId: string;
}
async function createEagerTab(): Promise<NewTab> {
  const r = await fetch(`${API_BASE}/api/tabs`, {
    method: "POST",
    headers: JSON_H,
    body: JSON.stringify({ eager: true }),
  });
  if (!r.ok) throw new Error(`POST /api/tabs -> ${r.status}`);
  const b = (await r.json()) as { tab_id: string; tab_id: string };
  return { tabId: b.tab_id, tabId: b.tab_id };
}
async function closeTab(id: string): Promise<void> {
  await fetch(`${API_BASE}/api/tabs/${id}/close`, { method: "POST", headers: AUTH_H }).catch(() => {});
}
async function muxHasTab(id: string): Promise<boolean> {
  const r = await fetch(`${API_BASE}/api/mux/state`, { headers: AUTH_H });
  const s = (await r.json()) as { tabs?: Record<string, unknown> };
  return Boolean(s.tabs?.[id]);
}
async function eventStats(): Promise<Record<string, number | boolean>> {
  const r = await fetch(`${API_BASE}/api/debug/events/stats`, { headers: AUTH_H });
  if (!r.ok) throw new Error(`/api/debug/events/stats -> ${r.status} (uninstrumented build?)`);
  return (await r.json()) as Record<string, number | boolean>;
}
async function jsReceived(page: Page): Promise<number> {
  return page.evaluate(() => (window as unknown as { __ctEvents?: { received: number } }).__ctEvents?.received ?? 0);
}
async function forceMuxStreamBreak(page: Page): Promise<number> {
  await page.waitForFunction(() => typeof (
    window as unknown as { __ctEvents?: { breakMuxStream?: () => void } }
  ).__ctEvents?.breakMuxStream === "function", { timeout: 5_000 });
  return page.evaluate(() => {
    const stats = (window as unknown as {
      __ctEvents?: { reconnects: number; breakMuxStream?: () => void };
    }).__ctEvents;
    if (!stats?.breakMuxStream) throw new Error("mux stream fault injection is unavailable");
    stats.breakMuxStream();
    return stats.reconnects;
  });
}
/** Wait up to `ms` for the tab's TAB to appear in the sidebar DOM. */
async function tabInSidebar(page: Page, tabId: string, ms: number): Promise<boolean> {
  const deadline = Date.now() + ms;
  do {
    if ((await page.locator(`[data-tab-id="${tabId}"]`).count()) > 0) return true;
    await page.waitForTimeout(150);
  } while (Date.now() < deadline);
  return false;
}

test("@quarantine task_uwkt7p event-freeze: forced latch reconciles sidebar, reset recovers", async () => {
  test.setTimeout(120_000);
  const { browser, page } = await connectUi();
  try {
    // Sanity: the sidebar oracle surface exists (some tabs already rendered).
    expect(await page.locator("[data-tab-id]").count(), "sidebar must render tabs").toBeGreaterThan(0);

    // Baseline: a fresh eager tab's tab reaches the sidebar.
    const base = await createEagerTab();
    expect(await tabInSidebar(page, base.tabId, 8_000), "baseline: tab should reach sidebar").toBe(true);
    await closeTab(base.tabId);

    // Inject the latch: the emit path freezes, but bounded reconcile must still
    // make the backend-owned tab visible instead of leaving a silent divergence.
    await fetch(`${API_BASE}/api/debug/events/force-latch`, { method: "POST", headers: AUTH_H });
    const rxBefore = await jsReceived(page);
    const frozen = await createEagerTab();
    const appeared = await tabInSidebar(page, frozen.tabId, 12_000);
    const inMux = await muxHasTab(frozen.tabId);
    const stats = await eventStats();
    const rxAfter = await jsReceived(page);
    console.log(`[freeze] appeared=${appeared} inMux=${inMux} rxΔ=${rxAfter - rxBefore} stats=${JSON.stringify(stats)}`);
    expect(inMux, "latched: mux still has the tab").toBe(true);
    expect(stats.frozen, "latched: emit path frozen").toBe(true);
    expect(appeared, "latched: sidebar must reconcile the new tab").toBe(true);

    // Reset the latch: the next emit path remains healthy.
    await fetch(`${API_BASE}/api/debug/events/reset-latch`, { method: "POST", headers: AUTH_H });
    const recovered = await createEagerTab();
    expect(await tabInSidebar(page, recovered.tabId, 8_000), "reset: sidebar applies events again").toBe(true);
    await closeTab(recovered.tabId);
    await closeTab(frozen.tabId);
  } finally {
    // Always clear the latch, even if an assertion above threw — a leaked
    // frozen=true would poison every later test against this dev.
    await fetch(`${API_BASE}/api/debug/events/reset-latch`, { method: "POST", headers: AUTH_H }).catch(() => {});
    await browser.close();
  }
});

test("@quarantine task_uwkt7p event-freeze: broken desktop stream reconnects and reconciles sidebar", async () => {
  test.setTimeout(30_000);
  const { browser, page } = await connectUi();
  let tab: NewTab | null = null;
  try {
    expect(await page.locator("[data-tab-id]").count(), "sidebar must render tabs").toBeGreaterThan(0);
    const reconnectsBefore = await forceMuxStreamBreak(page);
    tab = await createEagerTab();
    expect(await tabInSidebar(page, tab.tabId, 5_000), "broken stream must initially miss the tab").toBe(false);
    expect(await muxHasTab(tab.tabId), "daemon must contain the tab during divergence").toBe(true);
    expect(await tabInSidebar(page, tab.tabId, 12_000), "renewal must reconcile the tab into the DOM").toBe(true);
    const reconnectsAfter = await page.evaluate(() => (
      window as unknown as { __ctEvents?: { reconnects: number } }
    ).__ctEvents?.reconnects ?? 0);
    expect(reconnectsAfter, "desktop mux subscription must be renewed").toBeGreaterThan(reconnectsBefore);
  } finally {
    if (tab) await closeTab(tab.tabId);
    await browser.close();
  }
});

test("@quarantine task_uwkt7p event-freeze: 500x tab soak keeps sidebar applying events", async () => {
  test.setTimeout(30 * 60_000);
  const { browser, page } = await connectUi();
  try {
    // Guard against a leaked latch from an earlier aborted run.
    await fetch(`${API_BASE}/api/debug/events/reset-latch`, { method: "POST", headers: AUTH_H }).catch(() => {});
    const probe = async (label: string) => {
      const p = await createEagerTab();
      const alive = await tabInSidebar(page, p.tabId, 8_000);
      const inMux = await muxHasTab(p.tabId);
      const stats = await eventStats();
      const rx = await jsReceived(page);
      console.log(`[probe:${label}] alive=${alive} inMux=${inMux} rx=${rx} stats=${JSON.stringify(stats)}`);
      await closeTab(p.tabId);
      expect(alive, `${label}: sidebar did not apply tab ${p.tabId} (mux had tab: ${inMux})`).toBe(true);
    };
    await probe("baseline");
    for (let i = 1; i <= CYCLES; i++) {
      const p = await createEagerTab();
      await closeTab(p.tabId);
      if (i % PROBE_EVERY === 0) await probe(`cycle-${i}`);
    }
  } finally {
    await browser.close();
  }
});
