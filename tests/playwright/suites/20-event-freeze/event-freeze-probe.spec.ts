/**
 * Desktop event-freeze probe (task_uwkt7p). @quarantine so broad golden/full
 * sweeps skip it — run explicitly by file.
 *
 * "Window stops applying daemon events": create an eager pane via POST
 * /api/panes, poll the desktop DOM over CDP for its [data-pane-id]. A frozen
 * window never gets the pane while /api/mux/state still lists it.
 *
 * Test 1 (deterministic): force the `mux_emit_pending` latch via the debug
 * fault-injection route and prove the desktop stops applying events, then reset
 * and prove it recovers — this pins the mechanism without waiting for the soak
 * to exhaust OS threads.
 * Test 2 (natural trigger): 500 create/close cycles, probing every 50.
 *
 * Counters read both ends: Rust /api/debug/events/stats vs frontend
 * window.__ctEvents, so a stall is classified emit-side latch vs channel death.
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

async function createEagerPane(): Promise<string> {
  const r = await fetch(`${API_BASE}/api/panes`, {
    method: "POST",
    headers: JSON_H,
    body: JSON.stringify({ eager: true }),
  });
  if (!r.ok) throw new Error(`POST /api/panes -> ${r.status}`);
  return ((await r.json()) as { pane_id: string }).pane_id;
}
async function closePane(id: string): Promise<void> {
  await fetch(`${API_BASE}/api/panes/${id}/close`, { method: "POST", headers: AUTH_H }).catch(() => {});
}
async function muxHasPane(id: string): Promise<boolean> {
  const r = await fetch(`${API_BASE}/api/mux/state`, { headers: AUTH_H });
  const s = (await r.json()) as { panes?: Record<string, unknown> };
  return Boolean(s.panes?.[id]);
}
async function eventStats(): Promise<Record<string, number | boolean>> {
  const r = await fetch(`${API_BASE}/api/debug/events/stats`, { headers: AUTH_H });
  if (!r.ok) throw new Error(`/api/debug/events/stats -> ${r.status} (uninstrumented build?)`);
  return (await r.json()) as Record<string, number | boolean>;
}
async function jsReceived(page: Page): Promise<number> {
  return page.evaluate(() => (window as unknown as { __ctEvents?: { received: number } }).__ctEvents?.received ?? 0);
}
/** Wait up to `ms` for the pane to enter the desktop DOM. */
async function paneInDom(page: Page, id: string, ms: number): Promise<boolean> {
  const deadline = Date.now() + ms;
  do {
    if ((await page.locator(`[data-pane-id="${id}"]`).count()) > 0) return true;
    await page.waitForTimeout(150);
  } while (Date.now() < deadline);
  return false;
}

test("@quarantine event-freeze: forced latch freezes desktop emits, reset recovers", async () => {
  test.setTimeout(120_000);
  const { browser, page } = await connectUi();
  try {
    // Baseline: a fresh eager pane reaches the DOM.
    const base = await createEagerPane();
    expect(await paneInDom(page, base, 8_000), "baseline: pane should reach DOM").toBe(true);
    await closePane(base);

    // Inject the latch, then a new pane must NOT reach the DOM though mux has it.
    await fetch(`${API_BASE}/api/debug/events/force-latch`, { method: "POST", headers: AUTH_H });
    const rxBefore = await jsReceived(page);
    const frozen = await createEagerPane();
    const appeared = await paneInDom(page, frozen, 5_000);
    const inMux = await muxHasPane(frozen);
    const stats = await eventStats();
    const rxAfter = await jsReceived(page);
    console.log(`[freeze] appeared=${appeared} inMux=${inMux} rxΔ=${rxAfter - rxBefore} stats=${JSON.stringify(stats)}`);
    expect(inMux, "latched: mux still has the pane").toBe(true);
    expect(appeared, "latched: desktop must NOT apply the pane").toBe(false);
    expect(stats.pending, "latched: pending stuck true").toBe(true);

    // Reset the latch: the SAME pane's next emit path recovers.
    await fetch(`${API_BASE}/api/debug/events/reset-latch`, { method: "POST", headers: AUTH_H });
    const recovered = await createEagerPane();
    expect(await paneInDom(page, recovered, 8_000), "reset: desktop applies events again").toBe(true);
    await closePane(recovered);
    await closePane(frozen);
  } finally {
    await browser.close();
  }
});

test("@quarantine event-freeze: 500x pane soak keeps desktop applying events", async () => {
  test.setTimeout(30 * 60_000);
  const { browser, page } = await connectUi();
  try {
    const probe = async (label: string) => {
      const id = await createEagerPane();
      const alive = await paneInDom(page, id, 8_000);
      const inMux = await muxHasPane(id);
      const stats = await eventStats();
      const rx = await jsReceived(page);
      console.log(`[probe:${label}] alive=${alive} inMux=${inMux} rx=${rx} stats=${JSON.stringify(stats)}`);
      await closePane(id);
      expect(alive, `${label}: window did not apply pane ${id} (mux had it: ${inMux})`).toBe(true);
    };
    await probe("baseline");
    for (let i = 1; i <= CYCLES; i++) {
      const id = await createEagerPane();
      await closePane(id);
      if (i % PROBE_EVERY === 0) await probe(`cycle-${i}`);
    }
  } finally {
    await browser.close();
  }
});
