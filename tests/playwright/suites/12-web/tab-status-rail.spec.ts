/**
 * 12-web — tab/workspace status rail (real browser via webBrowser fixture).
 *
 * Attention / pending_prompt: driven via POST /api/panes/{id}/hook/permission
 * (production Claude hook path with explicit question+options — sets pane
 * pending_prompt server-side; not client-state poking). Waiting status alone
 * is covered by TabStatusRail.test.tsx precedence tests.
 */
import fs from "node:fs";
import path from "node:path";
import type { AppConfig } from "../../../../src/lib/types";
import { expect, test, type Browser, type Page } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";
import { REPO_ROOT } from "../../helpers/dev-console";
import { expandWorkspaceSidebar, openSidebarIfNeeded } from "../../helpers/canvas-inspect";
import { createManagedWebSession, type ManagedWebSession } from "../../helpers/web-session";

const RAILSHOT_DIR =
  "/private/tmp/claude-502/-Users-dev-Developer-codeterm-canvas/11e8775d-b50a-4688-8702-34672dbd1f4c/scratchpad/railshots";

const WORKING_LOOP =
  "bash -c 'i=0; while true; do echo \"pw-rail-$i\"; i=$((i+1)); sleep 0.2; done'";

type CreatedPane = { id: string; tab_id: string; workspace_id?: string };

function tab(page: Page, tabId: string) {
  return page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`);
}

function tabRail(page: Page, tabId: string) {
  return tab(page, tabId).locator('[data-testid="tab-status-rail"]');
}

function workspaceRail(page: Page, workspaceId: string) {
  return page
    .locator(`[data-testid="workspace-item"][data-workspace-id="${workspaceId}"]`)
    .locator('[data-testid="tab-status-rail"]');
}

async function paneStatus(
  api: { get<T>(p: string): Promise<T> },
  paneId: string,
): Promise<string> {
  const info = await api.get<{ status: string }>(`/panes/${paneId}/status`);
  return info.status;
}

async function waitPaneStatus(
  api: { get<T>(p: string): Promise<T> },
  paneId: string,
  want: string,
  label: string,
): Promise<void> {
  await pollUntil(async () => {
    const status = await paneStatus(api, paneId);
    return status === want ? status : undefined;
  }, 30_000, 500, label);
}

async function startWorking(api: { post(p: string, body?: unknown): Promise<unknown> }, paneId: string) {
  await api.post(`/panes/${paneId}/send`, { text: WORKING_LOOP, enter: true });
}

async function stopWorking(api: { post(p: string, body?: unknown): Promise<unknown> }, paneId: string) {
  await api.post(`/panes/${paneId}/send`, { text: "\x03", enter: false, raw: true });
}

async function driveAttentionPrompt(
  api: { post(p: string, body?: unknown): Promise<unknown> },
  paneId: string,
  question = "Allow pw-rail attention probe?",
) {
  await api.post(`/panes/${paneId}/hook/permission`, {
    question,
    options: [
      { label: "Yes", value: "1" },
      { label: "No", value: "2" },
    ],
  });
}

async function waitPendingPrompt(
  api: { get<T>(p: string): Promise<T> },
  paneId: string,
  question: string,
): Promise<void> {
  await pollUntil(async () => {
    const mux = await api.get<{
      panes: Record<string, { pending_prompt?: { question?: string } | null }>;
    }>("/mux/state");
    const prompt = mux.panes[paneId]?.pending_prompt;
    return prompt?.question === question ? prompt : undefined;
  }, 20_000, 500, `pending_prompt for ${paneId}`);
}

async function expectRailToken(
  page: Page,
  rail: ReturnType<typeof tabRail>,
  token: "--ct-green" | "--ct-yellow" | "--ct-red",
) {
  const bg = await rail.evaluate((el) => getComputedStyle(el).backgroundColor);
  const expected = await page.evaluate((tokenName) => {
    const probe = document.createElement("span");
    probe.style.background = `var(${tokenName})`;
    document.body.appendChild(probe);
    const color = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return color;
  }, token);
  expect(bg, `rail background should resolve ${token}`).toBe(expected);
}

async function setTheme(api: { request<T>(m: string, p: string, body?: unknown): Promise<T> }, config: AppConfig, name: string) {
  await api.request<AppConfig>("PATCH", "/config", { ...config, theme: { ...config.theme, name } });
}

async function withWeb(
  webBrowser: Browser,
  fn: (session: ManagedWebSession) => Promise<void>,
): Promise<void> {
  const session = await createManagedWebSession(webBrowser);
  try {
    await fn(session);
  } finally {
    await session.close();
  }
}

test.describe("@golden tab status rail", () => {
  test.describe.configure({ mode: "serial" });

  test("TabStatusRail module stays platform-blind (no IS_WEB)", () => {
    const src = fs.readFileSync(path.join(REPO_ROOT, "src/components/Sidebar/TabStatusRail.tsx"), "utf8");
    expect(src).not.toContain("IS_WEB");
  });

  test("working pane → green breathing rail on tab", async ({ api, webBrowser, paneCleanup }) => {
    test.setTimeout(120_000);
    await withWeb(webBrowser, async ({ page }) => {
      const pane = await paneCleanup.createPane<CreatedPane>({ title: "pw-rail-working" });
      await startWorking(api, pane.id);
      await waitPaneStatus(api, pane.id, "Working", "pane reaches Working");

      const rail = tabRail(page, pane.tab_id);
      await expect(rail).toBeVisible({ timeout: 20_000 });
      await expect(rail).toHaveClass(/ct-tab-status-rail--working/);
      await expect(rail).toHaveAttribute("data-rail-variant", "working");
      await expectRailToken(page, rail, "--ct-green");

      const anim = await rail.evaluate((el) => getComputedStyle(el).animationName);
      expect(anim).toContain("ct-tab-rail-breathe");

      await stopWorking(api, pane.id);
    });
  });

  test("working→idle → settle flash once, terminal state has no rail", async ({ api, webBrowser, paneCleanup }) => {
    test.setTimeout(120_000);
    await withWeb(webBrowser, async ({ page }) => {
      const pane = await paneCleanup.createPane<CreatedPane>({ title: "pw-rail-settle" });
      await startWorking(api, pane.id);
      await waitPaneStatus(api, pane.id, "Working", "pane reaches Working");
      await expect(tabRail(page, pane.tab_id)).toBeVisible({ timeout: 20_000 });

      const settlePromise = page.evaluate((tabId) => {
        return new Promise<boolean>((resolve) => {
          const row = document.querySelector(`[data-tab-id="${tabId}"]`);
          if (!row) {
            resolve(false);
            return;
          }
          let seen = false;
          const obs = new MutationObserver(() => {
            if (row.querySelector('[data-rail-variant="settle"]')) seen = true;
          });
          obs.observe(row, {
            subtree: true,
            childList: true,
            attributes: true,
            attributeFilter: ["data-rail-variant", "class"],
          });
          setTimeout(() => {
            obs.disconnect();
            resolve(seen);
          }, 2_500);
        });
      }, pane.tab_id);

      await stopWorking(api, pane.id);
      await waitPaneStatus(api, pane.id, "Idle", "pane reaches Idle after stop");
      const sawSettle = await settlePromise;
      expect(sawSettle, "Working→Idle should fire one settle flash").toBe(true);
      await expect(tabRail(page, pane.tab_id)).toHaveCount(0, { timeout: 5_000 });
    });
  });

  test("idle pane → no status rail", async ({ api, webBrowser, paneCleanup }) => {
    test.setTimeout(90_000);
    await withWeb(webBrowser, async ({ page }) => {
      const pane = await paneCleanup.createPane<CreatedPane>({ title: "pw-rail-idle" });
      await waitPaneStatus(api, pane.id, "Idle", "fresh pane settles idle");
      await expect(tabRail(page, pane.tab_id)).toHaveCount(0, { timeout: 10_000 });
    });
  });

  test("RAIL-3 attention via hook driver → yellow rail + status dot", async ({ api, webBrowser, paneCleanup }) => {
    test.setTimeout(60_000);
    const question = "Allow pw-rail attention probe (RAIL-3)?";
    await withWeb(webBrowser, async ({ page }) => {
      const pane = await paneCleanup.createPane<CreatedPane>({ title: "pw-rail-attn-3" });
      // Settle the spawn-transient "Working" status first so clearing the
      // prompt later lands on a quiescent pane (no rail), not a race against
      // the fresh-PTY status still winding down.
      await waitPaneStatus(api, pane.id, "Idle", "pane settles idle before attention hook");
      await openSidebarIfNeeded(page);
      if (pane.workspace_id) {
        await expandWorkspaceSidebar(page, pane.workspace_id);
      }

      await driveAttentionPrompt(api, pane.id, question);
      await waitPendingPrompt(api, pane.id, question);

      const row = tab(page, pane.tab_id);
      const rail = tabRail(page, pane.tab_id);
      await expect(rail).toBeVisible({ timeout: 5_000 });
      await expect(rail).toHaveAttribute("data-rail-variant", "attention");
      await expect(rail).toHaveClass(/ct-tab-status-rail--attention/);
      await expectRailToken(page, rail, "--ct-yellow");
      await expect(row.locator(`span.rounded-full[title="${question}"]`)).toHaveCount(1);

      await api.post(`/panes/${pane.id}/prompt/clear`, {});
      await expect(rail).toHaveCount(0, { timeout: 5_000 });
    });
  });

  test("RAIL-4 attention outranks working", async ({ api, webBrowser, paneCleanup }) => {
    test.setTimeout(90_000);
    const question = "Allow pw-rail attention probe (RAIL-4)?";
    await withWeb(webBrowser, async ({ page }) => {
      const pane = await paneCleanup.createPane<CreatedPane>({ title: "pw-rail-attn-4" });
      await startWorking(api, pane.id);
      await waitPaneStatus(api, pane.id, "Working", "pane reaches Working before permission hook");

      await openSidebarIfNeeded(page);
      if (pane.workspace_id) {
        await expandWorkspaceSidebar(page, pane.workspace_id);
      }
      const rail = tabRail(page, pane.tab_id);
      await expect(rail).toHaveAttribute("data-rail-variant", "working", { timeout: 20_000 });

      await driveAttentionPrompt(api, pane.id, question);
      await waitPendingPrompt(api, pane.id, question);

      // Output keeps flowing under the loop; attention must win the precedence
      // (Dead > Waiting/attention > Working) and must not get raced back to
      // "working" by the next chunk of ordinary shell output.
      await expect(rail).toHaveAttribute("data-rail-variant", "attention", { timeout: 5_000 });
      await expectRailToken(page, rail, "--ct-yellow");
      await page.waitForTimeout(1_000);
      await expect(rail).toHaveAttribute("data-rail-variant", "attention");

      await api.post(`/panes/${pane.id}/prompt/clear`, {});
      await expect(rail).toHaveAttribute("data-rail-variant", "working", { timeout: 5_000 });

      await stopWorking(api, pane.id);
    });
  });

  test("RAIL-11 attention survives reload", async ({ api, webBrowser, paneCleanup }) => {
    test.setTimeout(120_000);
    const question = "Allow pw-rail attention probe (RAIL-11)?";
    await withWeb(webBrowser, async ({ page }) => {
      const original = await api.get<AppConfig>("/config");
      const themes = await api.get<{ themes: string[] }>("/themes");
      const dark = themes.themes.find((t) => /dark/i.test(t)) ?? "github-dark";

      const pane = await paneCleanup.createPane<CreatedPane>({ title: "pw-rail-attn-11" });
      await startWorking(api, pane.id);
      await waitPaneStatus(api, pane.id, "Working", "pane reaches Working before permission hook");
      await driveAttentionPrompt(api, pane.id, question);
      await waitPendingPrompt(api, pane.id, question);

      await page.reload({ waitUntil: "domcontentloaded" });
      await page.locator('[data-testid="peer-picker-button"]').waitFor({ timeout: 30_000 });
      await openSidebarIfNeeded(page);
      if (pane.workspace_id) {
        await expandWorkspaceSidebar(page, pane.workspace_id);
      }

      const row = tab(page, pane.tab_id);
      const rail = tabRail(page, pane.tab_id);
      await expect(row).toBeVisible({ timeout: 20_000 });
      await expect(rail).toBeVisible({ timeout: 5_000 });
      await expect(rail).toHaveAttribute("data-rail-variant", "attention");
      await expect(rail).toHaveClass(/ct-tab-status-rail--attention/);
      await expectRailToken(page, rail, "--ct-yellow");
      await expect(row.locator(`span.rounded-full[title="${question}"]`)).toHaveCount(1);

      await setTheme(api, original, dark);
      await page.waitForTimeout(400);
      const attentionShot = path.join(RAILSHOT_DIR, "sidebar-attention-yellow.png");
      await page.screenshot({ path: attentionShot, fullPage: false });
      await setTheme(api, original, original.theme.name);

      expect(fs.existsSync(attentionShot), `missing screenshot ${attentionShot}`).toBe(true);
      expect(fs.statSync(attentionShot).size, `empty screenshot ${attentionShot}`).toBeGreaterThan(1_000);

      await api.post(`/panes/${pane.id}/prompt/clear`, {}).catch(() => {});
      await stopWorking(api, pane.id);
    });
  });

  test("dead pane → red rail on dimmed tab", async ({ api, webBrowser, paneCleanup }) => {
    test.setTimeout(90_000);
    await withWeb(webBrowser, async ({ page }) => {
      const pane = await paneCleanup.createPane<CreatedPane>({ title: "pw-rail-dead" });
      await api.post(`/panes/${pane.id}/send`, { text: "exit", enter: true });
      await waitPaneStatus(api, pane.id, "Dead", "pane reaches Dead after exit");

      const row = tab(page, pane.tab_id);
      const rail = tabRail(page, pane.tab_id);
      await expect(rail).toBeVisible({ timeout: 20_000 });
      await expect(rail).toHaveClass(/ct-tab-status-rail--dead/);
      await expect(rail).toHaveAttribute("data-rail-variant", "dead");
      await expectRailToken(page, rail, "--ct-red");
      await expect(row).toHaveClass(/sidebar-tab-dimmed/);
    });
  });

  test("workspace row mirrors aggregate status (working beats idle)", async ({ api, webBrowser, paneCleanup }) => {
    test.setTimeout(180_000);
    await withWeb(webBrowser, async ({ page }) => {
      const wsName = `pw-rail-ws-${Date.now()}`;
      await api.createWorkspace(wsName);
      const workspaces = await api.workspaces();
      const ws = workspaces.find((w) => w.name === wsName);
      expect(ws).toBeDefined();

      const idlePane = await paneCleanup.createPane<CreatedPane>({
        workspace_id: ws!.id,
        title: "pw-rail-ws-idle",
      });
      await waitPaneStatus(api, idlePane.id, "Idle", "idle pane in workspace");

      const workingPane = await paneCleanup.createPane<CreatedPane>({
        workspace_id: ws!.id,
        title: "pw-rail-ws-working",
      });
      await startWorking(api, workingPane.id);
      await waitPaneStatus(api, workingPane.id, "Working", "working pane in workspace");

      await api.focusWorkspace(ws!.id);
      await openSidebarIfNeeded(page);
      await expandWorkspaceSidebar(page, ws!.id);
      await expect(workspaceRail(page, ws!.id)).toBeVisible({ timeout: 20_000 });
      await expect(workspaceRail(page, ws!.id)).toHaveClass(/ct-tab-status-rail--working/);

      await stopWorking(api, workingPane.id);
      await api.closeWorkspace(ws!.id).catch(() => {});
    });
  });

  test("reduced-motion → static working rail (no breathe animation)", async ({ api, webBrowser, paneCleanup }) => {
    test.setTimeout(120_000);
    await withWeb(webBrowser, async ({ page }) => {
      const pane = await paneCleanup.createPane<CreatedPane>({ title: "pw-rail-reduced-motion" });
      await page.emulateMedia({ reducedMotion: "reduce" });
      await startWorking(api, pane.id);
      await waitPaneStatus(api, pane.id, "Working", "pane reaches Working");

      const rail = tabRail(page, pane.tab_id);
      await expect(rail).toBeVisible({ timeout: 20_000 });
      await expect(rail).toHaveClass(/ct-tab-status-rail--working/);

      const style = await rail.evaluate((el) => {
        const cs = getComputedStyle(el);
        return { animationName: cs.animationName, opacity: cs.opacity };
      });
      expect(style.animationName === "none" || style.animationName === "").toBe(true);
      expect(Number(style.opacity)).toBeCloseTo(0.85, 1);

      await page.emulateMedia({ reducedMotion: "no-preference" });
      await stopWorking(api, pane.id);
    });
  });

  test("visual railshots: mixed states, 10+ working, reduced-motion static", async ({ api, webBrowser, paneCleanup }) => {
    test.setTimeout(300_000);
    await withWeb(webBrowser, async ({ page }) => {
      const original = await api.get<AppConfig>("/config");
      const themes = await api.get<{ themes: string[] }>("/themes");
      const dark = themes.themes.find((t) => /dark/i.test(t)) ?? "github-dark";
      const light = themes.themes.find((t) => /light/i.test(t) && t !== dark) ?? "github-light";

      const wsName = `pw-rail-shots-${Date.now()}`;
      await api.createWorkspace(wsName);
      const ws = (await api.workspaces()).find((w) => w.name === wsName)!;
      await api.focusWorkspace(ws.id);

      const idlePane = await paneCleanup.createPane<CreatedPane>({
        workspace_id: ws.id,
        title: "pw-shot-idle",
      });
      await waitPaneStatus(api, idlePane.id, "Idle", "shot idle pane");

      const workingPane = await paneCleanup.createPane<CreatedPane>({
        workspace_id: ws.id,
        title: "pw-shot-working",
      });
      await startWorking(api, workingPane.id);

      const deadPane = await paneCleanup.createPane<CreatedPane>({
        workspace_id: ws.id,
        title: "pw-shot-dead",
      });
      await api.post(`/panes/${deadPane.id}/send`, { text: "exit", enter: true });
      await waitPaneStatus(api, deadPane.id, "Dead", "shot dead pane");

      const workingMany: CreatedPane[] = [];
      for (let i = 0; i < 11; i++) {
        const p = await paneCleanup.createPane<CreatedPane>({
          workspace_id: ws.id,
          title: `pw-shot-fleet-${i}`,
        });
        await startWorking(api, p.id);
        workingMany.push(p);
      }

      await openSidebarIfNeeded(page);
      await expandWorkspaceSidebar(page, ws.id);
      await expect(tabRail(page, workingPane.tab_id)).toBeVisible({ timeout: 30_000 });

      const mixedDark = path.join(RAILSHOT_DIR, "sidebar-mixed-dark.png");
      const mixedLight = path.join(RAILSHOT_DIR, "sidebar-mixed-light.png");
      const fleetShot = path.join(RAILSHOT_DIR, "sidebar-10plus-working-uniform-phase.png");
      const reducedShot = path.join(RAILSHOT_DIR, "sidebar-reduced-motion-static-rails.png");

      await setTheme(api, original, dark);
      await page.waitForTimeout(400);
      await page.screenshot({ path: mixedDark, fullPage: false });

      await setTheme(api, original, light);
      await page.waitForTimeout(400);
      await page.screenshot({ path: mixedLight, fullPage: false });

      await page.screenshot({ path: fleetShot, fullPage: false });

      await page.emulateMedia({ reducedMotion: "reduce" });
      await page.waitForTimeout(300);
      await page.screenshot({ path: reducedShot, fullPage: false });
      await page.emulateMedia({ reducedMotion: "no-preference" });

      for (const p of [workingPane, ...workingMany]) {
        await stopWorking(api, p.id).catch(() => {});
      }
      await setTheme(api, original, original.theme.name);
      await api.closeWorkspace(ws.id).catch(() => {});

      for (const shot of [mixedDark, mixedLight, fleetShot, reducedShot]) {
        expect(fs.existsSync(shot), `missing screenshot ${shot}`).toBe(true);
        expect(fs.statSync(shot).size, `empty screenshot ${shot}`).toBeGreaterThan(1_000);
      }
    });
  });
});
