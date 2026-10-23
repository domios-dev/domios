// Owner-requested opt-in: pnpm exec playwright test -c tests/playwright/sidebar-touch.config.ts
import { expect, test } from "@playwright/test";
import { writeFile } from "node:fs/promises";
import { devConnection } from "../../../../evals/_lib/connection.cjs";

test("iPad taps and touch/momentum callback replay preserve sidebar rows under status churn", async ({ page }, testInfo) => {
  const connection = devConnection({ apiPort: 7686, instance: "" });
  const health = await fetch(`${connection.api}/api/health`).then(response => response.json());
  expect(health.instance.api_port).toBe("7686");
  const base = process.env.SIDEBAR_TOUCH_URL ?? "http://127.0.0.1:1438/";
  expect(new URL(base).hostname).toBe("127.0.0.1");
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.addInitScript(() => {
    const probe = { events: [] as Array<{ edge: string; scrollTop: number }>, injected: 0 };
    Object.assign(window, { sidebarTouchProbe: probe });
    document.addEventListener("sidebar-scroll-debug", event => {
      probe.events.push((event as CustomEvent).detail);
    });
    const originalFetch = window.fetch.bind(window);
    window.fetch = async (...args) => {
      const response = await originalFetch(...args);
      if (!String(args[0]).includes("/api/events/stream") || !response.body) return response;
      const encoder = new TextEncoder();
      const stream = response.body.pipeThrough(new TransformStream<Uint8Array, Uint8Array>({
        start(controller) {
          Object.assign(window, { sidebarStatus: (tabId: string, status: string) => {
            controller.enqueue(encoder.encode(`data: ${JSON.stringify({ event_type: "tab-status-changed", data: { tab_id: tabId, status } })}\n\n`));
            probe.injected += 1;
          } });
        },
      }));
      return new Response(stream, { status: response.status, headers: response.headers });
    };
  });
  await page.goto(`${base}#token=${encodeURIComponent(connection.token)}`);
  await page.getByTestId("sidebar").waitFor();
  await page.getByRole("button", { name: "Select workspace codeterm", exact: true }).last().tap();
  await page.waitForFunction(() => document.querySelectorAll("[data-testid=sidebar] [data-tab-id]").length >= 30);
  await page.waitForFunction(() => "sidebarStatus" in window);
  const samples: Array<{ phase: string; before: number; intended: number; after: number; residual: number }> = [];
  const scroller = page.locator(".ct-sidebar-content");
  await scroller.evaluate(element => new Promise<void>(resolve => {
    element.addEventListener("scroll", () => resolve(), { once: true });
    element.scrollTop = 700;
  }));
  const tabId = await scroller.evaluate(element => {
    const bounds = element.getBoundingClientRect();
    return [...element.querySelectorAll<HTMLElement>("[data-tab-id]")].find(row => {
      const rect = row.getBoundingClientRect();
      return rect.top > bounds.top + 80 && rect.bottom < bounds.bottom - 80;
    })?.dataset.tabId;
  });
  expect(tabId, "many rows include a fully visible tap target").toBeTruthy();
  const row = page.locator(`[data-tab-id="${tabId}"]`).first();
  const activation = row.getByRole("button", { name: /^Open tab / }).first();
  await row.evaluate(element => Object.assign(element, { sidebarTouchIdentity: true }));
  await page.evaluate(() => {
    const probe = (window as unknown as { sidebarTouchProbe: { events: unknown[] } }).sidebarTouchProbe;
    probe.events = [];
  });
  const beforeTap = await row.boundingBox();
  const beforeScroll = await scroller.evaluate(element => element.scrollTop);
  await activation.tap();
  await expect(activation).toHaveAttribute("aria-current", "page");
  const afterTap = await row.boundingBox();
  expect(Math.abs(afterTap!.y - beforeTap!.y), "real WebKit tap leaves the existing row within 1px").toBeLessThanOrEqual(1);
  expect(await scroller.evaluate(element => element.scrollTop)).toBe(beforeScroll);

  // WebKit exposes native taps only; replay the compositor/observer ordering without claiming a native swipe.
  for (const [phase, delta, status] of [["touch", 35, "Waiting"], ["momentum", 22, "Error"], ["momentum", 13, "Waiting"]] as const) {
    const sample = await row.evaluate((element, { phase, delta, tabId, status }) => new Promise<{
      phase: string; before: number; intended: number; after: number; residual: number;
    }>(resolve => requestAnimationFrame(() => {
      const viewport = element.closest(".ct-sidebar-content")!;
      const emit = (name: string) => viewport.dispatchEvent(new TouchEvent(name, { bubbles: true }));
      if (phase === "touch") { emit("touchstart"); emit("touchmove"); }
      const before = viewport.scrollTop;
      const y = element.getBoundingClientRect().top;
      viewport.scrollBy(0, delta);
      const intended = viewport.scrollTop;
      (window as unknown as { sidebarStatus: (id: string, status: string) => void }).sidebarStatus(tabId!, status);
      const churn = document.createElement("span");
      churn.textContent = phase;
      churn.hidden = true;
      element.append(churn);
      queueMicrotask(() => {
        const after = viewport.scrollTop;
        const residual = element.getBoundingClientRect().top - y + intended - before;
        if (phase === "touch") emit("touchend");
        resolve({ phase, before, intended, after, residual });
        churn.remove();
      });
    })), { phase, delta, tabId, status });
    samples.push(sample);
    await expect(row.getByRole("status")).toHaveText(new RegExp(`tab status ${status}$`));
  }
  const probe = await page.evaluate(() => (window as unknown as {
    sidebarTouchProbe: { events: Array<{ edge: string }>; injected: number };
  }).sidebarTouchProbe);
  const evidencePath = testInfo.outputPath("evidence.json");
  await writeFile(evidencePath, JSON.stringify({ backendBuild: health.build_sha, samples, probe }, null, 2));
  await testInfo.attach("sidebar-touch-evidence", { path: evidencePath, contentType: "application/json" });
  expect(probe.injected).toBe(3);
  expect(await row.evaluate(element => (element as HTMLElement & { sidebarTouchIdentity?: boolean }).sidebarTouchIdentity)).toBe(true);
  expect(probe.events.filter(event => event.edge === "row-removed")).toEqual([]);
  expect(probe.events.filter(event => event.edge === "write")).toEqual([]);
  for (const sample of samples) {
    expect(sample.intended).toBeGreaterThan(sample.before);
    expect(Math.abs(sample.after - sample.intended), `${sample.phase}: observer preserves native position`).toBeLessThanOrEqual(1);
    expect(Math.abs(sample.residual), `${sample.phase}: existing row follows only intended scrolling`).toBeLessThanOrEqual(1);
  }
  expect(errors).toEqual([]);
});
