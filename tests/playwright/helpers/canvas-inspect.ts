/**
 * Canvas inspection helper — reusable Playwright primitives for driving and
 * visually auditing the v2 plan-review canvas (workspace + focus modes).
 *
 * macOS/Linux run in web-mode (Chromium against dev /); Windows over CDP.
 * Built for a long visual-iteration loop: open the board, pan/zoom/fit/focus,
 * capture full-board + per-region screenshots, and assert no-overlap/no-overflow
 * from real DOM bounding boxes. No app-internal hooks — pure UI driving.
 */
import { expect, type Page } from "@playwright/test";
import type { Browser } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { ApiClient } from "./api-client";
import { closeCanvasWebSession, openCanvasWebPage } from "./canvas-web-page";
import { CANVAS_ZOOM_INTENSITY } from "../../../src/components/Canvas/useCanvasViewport";

export const CANVAS_ART_DIR = path.resolve(__dirname, "..", "artifacts", "canvas");

export const PERF_FIXTURE_KEY = "canvas.perfFixtureN";
export const SUBSTRATE_V2_KEY = "canvas.substrateV2";

/** Board is ready when the blit canvas or legacy DOM plan node is visible (not empty). */
export const CANVAS_BOARD_SEL =
  '[data-testid="canvas-plan-node"], [data-testid="canvas-world-canvas"]';

export const CANVAS_BOARD_OR_EMPTY_SEL =
  `${CANVAS_BOARD_SEL}, [data-testid="canvas-empty"]`;

/** Scope queries to the visible board — hidden workspace mounts can duplicate presence nodes. */
export function visibleCanvasScope(page: Page) {
  return page.locator('[data-testid="canvas-view"]:visible').first();
}

// Must match useCanvasViewport.ts CANVAS_ZOOM_INTENSITY; scale = exp(-deltaY * INTENSITY).
const ZOOM_INTENSITY = CANVAS_ZOOM_INTENSITY;

export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface NodeBox {
  testid: string;
  id: string | null;
  box: Box;
}

const NODE_TESTIDS = [
  "canvas-plan-node",
  "canvas-spec-node",
  "canvas-task-card",
  "canvas-milestone-bubble",
  "canvas-orchestrator-group",
] as const;

function ensureArtDir(): void {
  fs.mkdirSync(CANVAS_ART_DIR, { recursive: true });
}

export interface RenderReadbackNode {
  id: string;
  kind: string;
  title: string;
}

interface MuxTabState {
  workspaces: Array<{ id: string; tabs: Array<{ id: string }> }>;
}

export async function openSidebarIfNeeded(page: Page): Promise<void> {
  const toggle = page.locator('[data-testid="web-sidebar-open"]');
  if (await toggle.isVisible().catch(() => false)) await toggle.click();
}

export async function expandWorkspaceSidebar(page: Page, workspaceId?: string): Promise<void> {
  const scope = workspaceId
    ? page.locator(`[data-testid="workspace-item"][data-workspace-id="${workspaceId}"]`)
    : page.locator('[data-testid="workspace-item"]').first();
  if (!(await scope.isVisible().catch(() => false))) return;
  const expand = scope.locator('button[title="Expand"]');
  if (await expand.isVisible().catch(() => false)) await expand.click().catch(() => {});
}

export async function closeWorkspaceTabs(api: ApiClient, workspaceId: string): Promise<void> {
  for (let pass = 0; pass < 8; pass++) {
    const mux = await api.get<MuxTabState>("/mux/state");
    const ws = mux.workspaces.find((w) => w.id === workspaceId);
    const tabs = [...(ws?.tabs ?? [])];
    if (tabs.length === 0) return;
    for (const tab of tabs) {
      await api.post(`/workspaces/${workspaceId}/tabs/${tab.id}/close`);
    }
  }
  const mux = await api.get<MuxTabState>("/mux/state");
  const remaining = mux.workspaces.find((w) => w.id === workspaceId)?.tabs.length ?? 0;
  expect(remaining, "workspace tabs closed for inline canvas").toBe(0);
}

export async function closeActiveWorkspaceTabs(api: ApiClient): Promise<void> {
  const mux = await api.get<MuxTabState & { active_workspace_id?: string }>("/mux/state");
  const activeId =
    mux.active_workspace_id ?? mux.workspaces.find((w) => (w as { is_active?: boolean }).is_active)?.id;
  if (activeId) await closeWorkspaceTabs(api, activeId);
}

export async function clearCanvasHarnessFlags(page: Page): Promise<void> {
  await page.addInitScript(([perfKey, subKey]) => {
    try {
      window.localStorage.removeItem(perfKey);
      window.localStorage.removeItem(subKey);
    } catch {
      /* non-fatal */
    }
  }, [PERF_FIXTURE_KEY, SUBSTRATE_V2_KEY]);
}

export async function seedPerfFixture(page: Page, taskCount = 24): Promise<void> {
  await page.addInitScript(([perfKey, subKey, n]) => {
    try {
      window.localStorage.setItem(perfKey, String(n));
      window.localStorage.setItem(subKey, "true");
    } catch {
      /* surfaced by wait helpers */
    }
  }, [PERF_FIXTURE_KEY, SUBSTRATE_V2_KEY, taskCount]);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
}

export async function seedPerfFixtureAndOpenInlineCanvas(
  browser: Browser,
  api: ApiClient,
  taskCount = 24,
): Promise<{ page: Page; workspaceId: string; close: () => Promise<void> }> {
  const projectPath = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-canvas-perf-"));
  await api.createWorkspace("canvas perf gate", projectPath);
  const workspace = (await api.workspaces()).find((w) => w.cwd === projectPath);
  if (!workspace) throw new Error("perf workspace was not listed");
  await api.focusWorkspace(workspace.id);
  await closeWorkspaceTabs(api, workspace.id);
  const session = await openCanvasWebPage(browser, { perfFixtureN: taskCount });
  await openInlineWorkspaceCanvas(session.page, workspace.id);
  return {
    page: session.page,
    workspaceId: workspace.id,
    close: () => closeCanvasWebSession(session),
  };
}

async function waitForBoardSurface(page: Page): Promise<void> {
  await page.locator(CANVAS_BOARD_OR_EMPTY_SEL).first().waitFor({ state: "visible", timeout: 30_000 });
  await page.waitForTimeout(450);
}

export async function readbackRenderNodes(page: Page): Promise<RenderReadbackNode[]> {
  return page.evaluate(
    () => (window as unknown as { __codetermCanvasRender?: { nodes: RenderReadbackNode[] } }).__codetermCanvasRender?.nodes ?? [],
  );
}

export async function waitForReadbackCounts(
  page: Page,
  plans: number,
  tasks: number,
  timeoutMs = 30_000,
): Promise<void> {
  await expect
    .poll(
      async () => {
        const render = await readbackRenderNodes(page);
        let planCount = render.filter((n) => n.kind === "plan").length;
        let taskCount = render.filter((n) => n.kind === "task").length;
        if (planCount < plans || taskCount < tasks) {
          const sceneCounts = await page.evaluate(() => {
            const reg = (window as unknown as {
              __codetermCanvasScene?: { list: () => string[]; get: (slot: string) => { scene?: { nodes: Array<{ kind: string }> } } | null };
            }).__codetermCanvasScene;
            for (const slot of reg?.list() ?? []) {
              const nodes = reg?.get(slot)?.scene?.nodes ?? [];
              return {
                plans: nodes.filter((n) => n.kind === "plan").length,
                tasks: nodes.filter((n) => n.kind === "task").length,
              };
            }
            return { plans: 0, tasks: 0 };
          });
          planCount = Math.max(planCount, sceneCounts.plans);
          taskCount = Math.max(taskCount, sceneCounts.tasks);
        }
        return { plans: planCount, tasks: taskCount };
      },
      { timeout: timeoutMs },
    )
    .toEqual({ plans, tasks });
}

export async function assertReadbackNodesOnScreen(page: Page, minPx = 8): Promise<void> {
  await expect
    .poll(async () => {
      return page.evaluate((min) => {
        const registry = (window as unknown as {
          __codetermCanvasScene?: {
            list: () => string[];
            get: (slot: string) => { projectedBoxes: () => Array<{ screenBox: { width: number; height: number } }> } | null;
          };
        }).__codetermCanvasScene;
        let visible = 0;
        for (const slot of registry?.list() ?? []) {
          for (const box of registry?.get(slot)?.projectedBoxes() ?? []) {
            if (box.screenBox.width >= min && box.screenBox.height >= min) visible++;
          }
        }
        return visible;
      }, minPx);
    })
    .toBeGreaterThan(0);
}

export async function openPanePlanCanvas(page: Page, paneId?: string): Promise<void> {
  const scope = paneId ? page.locator(`[data-pane-id="${paneId}"]`) : page;
  const taskStrip = scope.locator('[data-testid="worker-task-strip"]').first();
  await taskStrip.click({ force: true });
  await expect(page.locator('[data-testid="pane-plan-overlay"]')).toHaveCount(0);
  await page.locator('[data-testid="canvas-view"]').first().waitFor({ state: "visible", timeout: 30_000 });
  await page.locator(CANVAS_BOARD_OR_EMPTY_SEL).first().waitFor({ state: "visible", timeout: 30_000 });
  await page.waitForTimeout(450);
}

export async function closePanePlanCanvas(page: Page, _paneId?: string): Promise<void> {
  const btn = page.locator('[data-testid="canvas-toolbar-button"]');
  if ((await btn.count()) > 0 && (await btn.getAttribute("aria-pressed")) === "true") {
    await btn.click({ force: true }).catch(() => {});
  }
  await page.waitForTimeout(150);
  await expect(page.locator('[data-testid="pane-plan-overlay"]')).toHaveCount(0);
}

// Inline workspace canvas (mode=workspace) mounts when the active workspace has no tab.
export async function openInlineWorkspaceCanvas(page: Page, workspaceId?: string): Promise<void> {
  await openSidebarIfNeeded(page);
  await expect(page.locator('[data-testid="pane-plan-overlay"]')).toHaveCount(0);
  const view = page.locator('[data-testid="canvas-view"]').first();
  if (!(await view.isVisible().catch(() => false))) {
    if (workspaceId) await expandWorkspaceSidebar(page, workspaceId);
    const entry = workspaceId
      ? page.locator(`div:has([data-workspace-id="${workspaceId}"]) [data-testid="workspace-plan-entry"]`)
      : page.locator('[data-testid="workspace-plan-entry"]').first();
    if (await entry.isVisible().catch(() => false)) {
      await entry.click().catch(async () => {
        await entry.click({ force: true });
      });
    } else {
      const btn = page.locator('[data-testid="canvas-toolbar-button"]');
      if ((await btn.count()) > 0 && (await btn.getAttribute("aria-pressed")) !== "true") {
        await btn.click().catch(() => {});
      }
    }
  }
  await view.waitFor({ state: "visible", timeout: 30_000 });
  await waitForBoardSurface(page);
}

export async function openWorkspaceCanvasOverlay(page: Page): Promise<void> {
  await openSidebarIfNeeded(page);
  const btn = page.locator('[data-testid="canvas-toolbar-button"]');
  await btn.waitFor({ state: "visible", timeout: 10_000 });
  if ((await btn.getAttribute("aria-pressed")) !== "true") await btn.click();
  await page.locator('[data-testid="canvas-view"]').first().waitFor({ state: "visible", timeout: 30_000 });
  await waitForBoardSurface(page);
}

// Robust board-open for unattended gates (perf/ux). Prefer any workspace Plan row,
// then fall back to the titlebar canvas button.
export async function ensureCanvasBoard(page: Page): Promise<void> {
  await openSidebarIfNeeded(page);
  const view = page.locator('[data-testid="canvas-view"]').first();
  if (!(await view.isVisible().catch(() => false))) {
    const workspaces = page.locator('[data-testid="workspace-item"]');
    const wsCount = await workspaces.count();
    for (let i = 0; i < wsCount; i++) {
      const expand = workspaces.nth(i).locator('button[title="Expand"]');
      if (await expand.isVisible().catch(() => false)) await expand.click().catch(() => {});
    }
    const row = page.locator('[data-testid="workspace-plan-entry"]').first();
    if (await row.isVisible().catch(() => false)) {
      await row.click().catch(async () => {
        await row.click({ force: true }).catch(() => {});
      });
    } else {
      const btn = page.locator('[data-testid="canvas-toolbar-button"]');
      if ((await btn.count()) > 0 && (await btn.getAttribute("aria-pressed")) !== "true") {
        await btn.click().catch(() => {});
      }
    }
  }
  await view.waitFor({ state: "visible", timeout: 30_000 });
  await waitForBoardSurface(page);
}

export async function openCanvas(page: Page): Promise<void> {
  const inlineView = page.locator('[data-testid="canvas-view"]').first();
  if (await inlineView.isVisible().catch(() => false)) {
    await inlineView.waitFor({ state: "visible", timeout: 30_000 });
  } else {
    await ensureCanvasBoard(page);
    return;
  }
  await waitForBoardSurface(page);
}

export async function isEmpty(page: Page): Promise<boolean> {
  return (await page.locator('[data-testid="canvas-empty"]').count()) > 0;
}

// Plain wheel = two-finger pan in the v2 viewport hook.
export async function panBy(page: Page, dx: number, dy: number): Promise<void> {
  const box = await page.locator('[data-testid="canvas-view"]').first().boundingBox();
  if (!box) return;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(dx, dy);
  await page.waitForTimeout(140);
}

// Cmd/Ctrl + wheel = zoom-around-pointer. Playwright's mouse.wheel can't set
// modifiers, so dispatch a real WheelEvent with ctrlKey on the pan container.
export async function zoomBy(page: Page, deltaY: number): Promise<void> {
  await page.evaluate((dy) => {
    const view = document.querySelector('[data-testid="canvas-view"]');
    const target = view?.querySelector("div");
    if (!target) return;
    const r = target.getBoundingClientRect();
    target.dispatchEvent(
      new WheelEvent("wheel", {
        deltaY: dy,
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
        clientX: r.left + r.width / 2,
        clientY: r.top + r.height / 2,
      }),
    );
  }, deltaY);
  await page.waitForTimeout(140);
}

// Dispatch a cmd-wheel zoom anchored on the center of the first element matching
// `anchorSel` (so it stays under the pointer); fall back to the viewport center
// when nothing matches. worlds-v3 puts tasks in a cloud RIGHT of the plan, so a
// plain viewport-center zoom drifts content off-screen and culls it.
async function dispatchZoom(page: Page, anchorSel: string | null, deltaY: number): Promise<void> {
  await page.evaluate(({ anchorSel, deltaY }) => {
    const view = document.querySelector('[data-testid="canvas-view"]');
    const target = view?.querySelector("div");
    if (!target) return;
    const anchor = anchorSel ? (document.querySelector(anchorSel) as HTMLElement | null) : null;
    const r = (anchor ?? target).getBoundingClientRect();
    target.dispatchEvent(
      new WheelEvent("wheel", {
        deltaY,
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
        clientX: r.left + r.width / 2,
        clientY: r.top + r.height / 2,
      }),
    );
  }, { anchorSel, deltaY });
  await page.waitForTimeout(150);
}

export async function zoomTowardTask(page: Page, taskId: string, deltaY: number): Promise<void> {
  await dispatchZoom(page, `[data-testid="canvas-task-card"][data-task-id="${taskId}"]`, deltaY);
}

// Absolute-scale zoom anchored on the first element matching `anchorSel`, keeping
// it on-screen across the dive. Falls back to viewport center while it is culled.
export async function setCanvasScaleOn(page: Page, anchorSel: string, targetScale: number): Promise<void> {
  for (let i = 0; i < 5; i++) {
    const current = await canvasScale(page);
    if (Math.abs(current - targetScale) < 0.02) return;
    const deltaY = -Math.log(targetScale / current) / ZOOM_INTENSITY;
    await dispatchZoom(page, anchorSel, deltaY);
    await page.waitForTimeout(30);
  }
}

export async function setCanvasScaleOnTask(page: Page, taskId: string, targetScale: number): Promise<void> {
  return setCanvasScaleOn(page, `[data-testid="canvas-task-card"][data-task-id="${taskId}"]`, targetScale);
}

// Drive the PRODUCT navigation method: POST /api/canvas/focus to land the camera
// on a node by id (centered + blink). Deterministic — no pan/zoom heuristics. The
// page already holds the dev token (connectUi seeds it). Optional scale lands at an
// exact zoom (needs backend scale passthrough; auto-fits otherwise).
export async function focusTaskById(page: Page, taskId: string, scale?: number): Promise<void> {
  await page.evaluate(async ({ id, scale }) => {
    const token = localStorage.getItem(`codeterm-web-token:${window.location.origin}`) || "";
    await fetch("/api/canvas/focus", {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
      body: JSON.stringify(scale != null ? { task_id: id, scale } : { task_id: id }),
    });
  }, { id: taskId, scale });
  await page.waitForTimeout(700);
}

// Read the rendered plan node's id, fetch its tasks, return an in_progress task id
// (else the first) — a stable probe without needing a card already on-screen.
export async function pickProbeTaskId(page: Page): Promise<string | null> {
  const planId = await page.getAttribute('[data-testid="canvas-plan-node"]', "data-plan-id").catch(() => null);
  if (!planId) return null;
  return page.evaluate(async (pid) => {
    const token = localStorage.getItem(`codeterm-web-token:${window.location.origin}`) || "";
    const r = await fetch(`/api/plans/${pid}`, { headers: { Authorization: `Bearer ${token}` } });
    if (!r.ok) return null;
    const p = await r.json();
    const tasks = (p.tasks || []) as Array<{ id: string; status: string }>;
    const t = tasks.find((x) => x.status === "in_progress") || tasks.find((x) => x.status !== "done") || tasks[0];
    return t?.id ?? null;
  }, planId);
}

// worlds-v3 places the task cloud spatially to the RIGHT of the plan (triptych:
// spec-left, plan-center, tasks-right). Fit-to-view frames the plan, so the cloud
// sits off-screen and its cards are culled. Mount them by PANNING toward the cloud
// at a mid zoom (where cards cross into a card LOD band), not by zooming. Returns
// the mounted cards (empty if none after the sweep).
export async function panToMountTaskCards(page: Page, midScale = 1.6): Promise<NodeBox[]> {
  await setCanvasScaleOn(page, '[data-testid="canvas-plan-node"]', midScale);
  let cards = await collectBoxes(page, "canvas-task-card");
  // The cloud spreads UP-RIGHT of the plan (phyllotaxis disk); sweep diagonally,
  // panning content down-left so the cloud scrolls into view, with vertical bands.
  const dyBands = [-160, -320, 0];
  for (const dy of dyBands) {
    for (let i = 0; i < 16 && cards.length === 0; i++) {
      await panBy(page, 300, i === 0 ? dy : -120);
      cards = await collectBoxes(page, "canvas-task-card");
    }
    if (cards.length > 0) break;
    // Re-center on the plan before trying the next band.
    await setCanvasScaleOn(page, '[data-testid="canvas-plan-node"]', midScale);
  }
  return cards;
}

// No fit-to-view button; close and reopen through the task strip to remount the
// focus canvas, which re-runs fit-to-view.
export async function fitToView(page: Page): Promise<void> {
  const strip = page.locator('[data-testid="worker-task-strip"]').first();
  if ((await strip.count()) === 0) return;
  await closePanePlanCanvas(page);
  await strip.click({ force: true });
  await page.locator('[data-testid="canvas-view"]').first().waitFor({ state: "visible", timeout: 15_000 });
  await page.waitForTimeout(400);
}

export async function focusFirstNode(page: Page, testid = "canvas-task-card"): Promise<void> {
  const node = page.locator(`[data-testid="${testid}"]`).first();
  if ((await node.count()) === 0) return;
  await node.scrollIntoViewIfNeeded().catch(() => {});
  await node.hover().catch(() => {});
  await page.waitForTimeout(120);
}

export async function collectBoxes(page: Page, testid: string): Promise<NodeBox[]> {
  return page.$$eval(
    `[data-testid="${testid}"]`,
    (els, tid) =>
      els.map((el) => {
        const r = el.getBoundingClientRect();
        return {
          testid: tid as string,
          id:
            el.getAttribute("data-plan-id") ??
            el.getAttribute("data-task-id") ??
            el.getAttribute("data-milestone-id") ??
            el.getAttribute("data-owner-key"),
          box: { x: r.x, y: r.y, width: r.width, height: r.height },
        };
      }),
    testid,
  );
}

function overlapArea(a: Box, b: Box): number {
  const x = Math.max(0, Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x));
  const y = Math.max(0, Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y));
  return x * y;
}

export interface OverlapHit {
  a: NodeBox;
  b: NodeBox;
  area: number;
}

// Cards (plan/spec/task) must never overlap each other — that was the v1 failure.
// Milestone bubbles and group wrappers are excluded (clusters/containers overlap by design).
export async function findOverlaps(page: Page, tolerancePx = 2): Promise<OverlapHit[]> {
  const groups = await Promise.all(
    ["canvas-plan-node", "canvas-spec-node", "canvas-task-card"].map((t) => collectBoxes(page, t)),
  );
  const all = groups.flat();
  const hits: OverlapHit[] = [];
  const minArea = tolerancePx * tolerancePx;
  for (let i = 0; i < all.length; i++) {
    for (let j = i + 1; j < all.length; j++) {
      const area = overlapArea(all[i].box, all[j].box);
      if (area > minArea) hits.push({ a: all[i], b: all[j], area });
    }
  }
  return hits;
}

export interface OverflowHit {
  testid: string;
  id: string | null;
  overflowY: number;
  overflowX: number;
}

export interface ContentBoundsHit {
  testid: string;
  id: string | null;
  childTestid: string;
  reason: string;
}

export interface FillHit {
  testid: string;
  id: string | null;
  ratio: number;
}

export interface EdgeEndpointHit {
  path: string;
  endpoint: "start" | "end";
  x: number;
  y: number;
}

// Content must size to its box (refinement §3): no card clips its content.
// Spec node scrolls by design, so it is excluded.
export async function findOverflows(page: Page, tolerancePx = 2): Promise<OverflowHit[]> {
  const out: OverflowHit[] = [];
  for (const testid of ["canvas-plan-node", "canvas-task-card"]) {
    const hits = await page.$$eval(
      `[data-testid="${testid}"]`,
      (els, tid) =>
        els
          .map((el) => ({
            testid: tid as string,
            id: el.getAttribute("data-plan-id") ?? el.getAttribute("data-task-id"),
            overflowY: el.scrollHeight - el.clientHeight,
            overflowX: el.scrollWidth - el.clientWidth,
          }))
          .filter((h) => h.overflowY > 2 || h.overflowX > 2),
      testid,
    );
    out.push(...hits.filter((h) => h.overflowY > tolerancePx || h.overflowX > tolerancePx));
  }
  return out;
}

export async function captureBoard(page: Page, label: string): Promise<string> {
  ensureArtDir();
  const file = path.join(CANVAS_ART_DIR, `${label}.png`);
  await page.locator('[data-testid="canvas-view"]').first().screenshot({ path: file }).catch(async () => {
    await page.screenshot({ path: file });
  });
  return file;
}

export async function captureRegions(page: Page, label: string): Promise<string[]> {
  ensureArtDir();
  const files: string[] = [];
  for (const testid of NODE_TESTIDS) {
    const first = page.locator(`[data-testid="${testid}"]`).first();
    if ((await first.count()) === 0) continue;
    const file = path.join(CANVAS_ART_DIR, `${label}-${testid}.png`);
    await first.screenshot({ path: file }).catch(() => {});
    files.push(file);
  }
  return files;
}

export async function canvasScale(page: Page): Promise<number> {
  return page.evaluate(() => {
    const view = document.querySelector('[data-testid="canvas-view"]');
    const world = Array.from(view?.querySelectorAll<HTMLElement>("div") ?? []).find((el) =>
      (el.getAttribute("style") ?? "").includes("transform: translate"),
    );
    const transform = world ? getComputedStyle(world).transform : null;
    if (!transform || transform === "none") return 1;
    const matrix = transform.match(/^matrix\(([^)]+)\)$/);
    if (matrix) {
      const [a, b] = matrix[1].split(",").map((v) => Number(v.trim()));
      return Math.sqrt(a * a + b * b) || 1;
    }
    const scale = transform.match(/scale\(([^)]+)\)/);
    return scale ? Number(scale[1]) : 1;
  });
}

export async function setCanvasScale(page: Page, targetScale: number): Promise<void> {
  for (let i = 0; i < 4; i++) {
    const current = await canvasScale(page);
    if (Math.abs(current - targetScale) < 0.015) return;
    await page.evaluate(({ currentScale, nextScale, intensity }) => {
      const view = document.querySelector('[data-testid="canvas-view"]');
      const target = view?.querySelector("div");
      if (!target) return;
      const r = target.getBoundingClientRect();
      const deltaY = -Math.log(nextScale / currentScale) / intensity;
      target.dispatchEvent(
        new WheelEvent("wheel", {
          deltaY,
          ctrlKey: true,
          bubbles: true,
          cancelable: true,
          clientX: r.left + r.width / 2,
          clientY: r.top + r.height / 2,
        }),
      );
    }, { currentScale: current, nextScale: targetScale, intensity: ZOOM_INTENSITY });
    await page.waitForTimeout(180);
  }
}

export async function findContentBoundsIssues(page: Page, tolerancePx = 2): Promise<ContentBoundsHit[]> {
  return page.$$eval(
    '[data-testid="canvas-spec-section"], [data-testid="canvas-task-card"]',
    (nodes, tolerance) => {
      const hits: ContentBoundsHit[] = [];
      const within = (a: DOMRect, b: DOMRect) =>
        a.left + tolerance >= b.left &&
        a.top + tolerance >= b.top &&
        a.right - tolerance <= b.right &&
        a.bottom - tolerance <= b.bottom;
      const visibleRect = (el: HTMLElement, host: HTMLElement) => {
        const r = el.getBoundingClientRect();
        const out = { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
        for (let p = el.parentElement; p && p !== host; p = p.parentElement) {
          const cs = getComputedStyle(p);
          const clipsX = /(auto|scroll|hidden|clip)/.test(cs.overflowX) && p.scrollWidth > p.clientWidth;
          const clipsY = /(auto|scroll|hidden|clip)/.test(cs.overflowY) && p.scrollHeight > p.clientHeight;
          if (!clipsX && !clipsY) continue;
          const pr = p.getBoundingClientRect();
          out.left = Math.max(out.left, pr.left);
          out.top = Math.max(out.top, pr.top);
          out.right = Math.min(out.right, pr.right);
          out.bottom = Math.min(out.bottom, pr.bottom);
          out.width = Math.max(0, out.right - out.left);
          out.height = Math.max(0, out.bottom - out.top);
        }
        return out;
      };
      for (const node of nodes) {
        const host = node as HTMLElement;
        const hostRect = host.getBoundingClientRect();
        const id = host.getAttribute("data-section-id") ?? host.getAttribute("data-task-id");
        const candidates = Array.from(
          host.querySelectorAll<HTMLElement>(
            '[data-testid="canvas-spec-section-body"], [data-testid="canvas-task-body"], [data-testid="canvas-task-intent"], [data-testid="canvas-task-why"]',
          ),
        );
        for (const child of candidates) {
          if ((child.textContent ?? "").trim().length === 0) continue;
          const rect = visibleRect(child, host);
          if (rect.height <= 0 || rect.width <= 0) {
            hits.push({
              testid: host.dataset.testid ?? "",
              id,
              childTestid: child.dataset.testid ?? "",
              reason: "zero-size-text-block",
            });
            continue;
          }
          if (!within(rect, hostRect)) {
            hits.push({
              testid: host.dataset.testid ?? "",
              id,
              childTestid: child.dataset.testid ?? "",
              reason: "content-outside-node",
            });
          }
        }
      }
      return hits;
    },
    tolerancePx,
  );
}

export async function findUnderfilledNodes(page: Page, minRatio = 0.004): Promise<FillHit[]> {
  // §22.2/§23.1: the plan node is intentionally compact in Tile/Card (a section
  // index, not an empty shell) — only task cards must carry visible content.
  return page.$$eval(
    '[data-testid="canvas-task-card"]',
    (nodes, threshold) => {
      const hits: FillHit[] = [];
      for (const node of nodes) {
        const host = node as HTMLElement;
        const box = host.getBoundingClientRect();
        const boxArea = Math.max(1, box.width * box.height);
        const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT);
        let textArea = 0;
        while (walker.nextNode()) {
          const text = walker.currentNode;
          if ((text.textContent ?? "").trim().length === 0) continue;
          const range = document.createRange();
          range.selectNodeContents(text);
          for (const rect of Array.from(range.getClientRects())) {
            textArea += Math.max(0, rect.width) * Math.max(0, rect.height);
          }
          range.detach();
        }
        const ratio = textArea / boxArea;
        if (ratio < threshold) {
          hits.push({
            testid: host.dataset.testid ?? "",
            id: host.getAttribute("data-plan-id") ?? host.getAttribute("data-section-id") ?? host.getAttribute("data-task-id"),
            ratio,
          });
        }
      }
      return hits;
    },
    minRatio,
  );
}

export async function findEdgeEndpointIssues(page: Page, tolerancePx = 3): Promise<EdgeEndpointHit[]> {
  return page.$$eval(
    '[data-testid="canvas-view"] svg.pointer-events-none path',
    (paths, tolerance) => {
      const anchors = Array.from(
        document.querySelectorAll<HTMLElement>(
          // §23.1: section→task edges run from a rail OUTPUT dot (inside the plan
          // node) to a task card; the spec→plan link runs node-center to node-center.
          // Both ends of every edge must land inside one of these nodes.
          '[data-testid="canvas-plan-node"], [data-testid="canvas-spec-node"], [data-testid="canvas-task-card"], [data-testid="canvas-milestone-bubble"]',
        ),
      ).map((el) => {
        const r = el.getBoundingClientRect();
        return {
          left: r.left - tolerance,
          top: r.top - tolerance,
          right: r.right + tolerance,
          bottom: r.bottom + tolerance,
        };
      });
      const circles = Array.from(document.querySelectorAll<SVGCircleElement>('[data-testid="canvas-view"] svg.pointer-events-none circle')).map((el) => {
        const r = el.getBoundingClientRect();
        return {
          left: r.left - tolerance,
          top: r.top - tolerance,
          right: r.right + tolerance,
          bottom: r.bottom + tolerance,
        };
      });
      const boxes = anchors.concat(circles);
      const inside = (x: number, y: number) => boxes.some((b) => x >= b.left && x <= b.right && y >= b.top && y <= b.bottom);
      const hits: EdgeEndpointHit[] = [];
      for (const path of paths as SVGPathElement[]) {
        // Orchestrator→plan connectors start at the FLOATING orchestrator avatar (not a
        // node anchor) by design — they aren't node-to-node edges, so skip them.
        if (path.getAttribute("data-testid") === "canvas-orchestrator-link") continue;
        const length = path.getTotalLength();
        if (!Number.isFinite(length) || length <= 0) continue;
        const start = path.getPointAtLength(0);
        const end = path.getPointAtLength(length);
        const ctm = path.getScreenCTM();
        if (!ctm) continue;
        const startScreen = new DOMPoint(start.x, start.y).matrixTransform(ctm);
        const endScreen = new DOMPoint(end.x, end.y).matrixTransform(ctm);
        if (!inside(startScreen.x, startScreen.y)) {
          hits.push({ path: path.getAttribute("d") ?? "", endpoint: "start", x: startScreen.x, y: startScreen.y });
        }
        if (!inside(endScreen.x, endScreen.y)) {
          hits.push({ path: path.getAttribute("d") ?? "", endpoint: "end", x: endScreen.x, y: endScreen.y });
        }
      }
      return hits;
    },
    tolerancePx,
  );
}

// One audit pass: capture + assert invariants. Returns counts for loop logging.
export interface AuditResult {
  nodes: number;
  overlaps: OverlapHit[];
  overflows: OverflowHit[];
  contentBounds: ContentBoundsHit[];
  underfilled: FillHit[];
  edgeEndpoints: EdgeEndpointHit[];
  board: string;
}

export async function auditCanvas(page: Page, label: string): Promise<AuditResult> {
  const board = await captureBoard(page, label);
  await captureRegions(page, label);
  const overlaps = await findOverlaps(page);
  const overflows = await findOverflows(page);
  const contentBounds = await findContentBoundsIssues(page);
  const underfilled = await findUnderfilledNodes(page);
  const edgeEndpoints = await findEdgeEndpointIssues(page);
  const planNodes = await collectBoxes(page, "canvas-plan-node");
  return { nodes: planNodes.length, overlaps, overflows, contentBounds, underfilled, edgeEndpoints, board };
}

export function assertClean(result: AuditResult): void {
  expect(result.overlaps, `overlapping cards: ${JSON.stringify(result.overlaps.map((h) => [h.a.testid, h.b.testid, Math.round(h.area)]))}`).toHaveLength(0);
  expect(result.overflows, `clipped/overflowing cards: ${JSON.stringify(result.overflows)}`).toHaveLength(0);
  expect(result.contentBounds, `bad content bounds: ${JSON.stringify(result.contentBounds)}`).toHaveLength(0);
  expect(result.underfilled, `underfilled nodes: ${JSON.stringify(result.underfilled)}`).toHaveLength(0);
  expect(result.edgeEndpoints, `edge endpoints outside anchors: ${JSON.stringify(result.edgeEndpoints)}`).toHaveLength(0);
}

export async function findVisibleOverflows(page: Page, rootSelector?: string, tolerancePx = 2): Promise<OverflowHit[]> {
  return page.evaluate(
    ({ rootSelector, tolerancePx }) => {
      const visible = (el: Element) => {
        const html = el as HTMLElement;
        const rect = html.getBoundingClientRect();
        if (rect.width < 2 || rect.height < 2) return false;
        const cs = getComputedStyle(html);
        return cs.display !== "none" && cs.visibility !== "hidden" && Number(cs.opacity) >= 0.05;
      };
      const roots = rootSelector
        ? Array.from(document.querySelectorAll(rootSelector))
        : Array.from(document.querySelectorAll('[data-testid="canvas-view"]'));
      const out: OverflowHit[] = [];
      for (const root of roots) {
        if (!visible(root)) continue;
        for (const testid of ["canvas-plan-node", "canvas-task-card"]) {
          for (const el of root.querySelectorAll(`[data-testid="${testid}"]`)) {
            if (!visible(el)) continue;
            const host = el as HTMLElement;
            const overflowY = host.scrollHeight - host.clientHeight;
            const overflowX = host.scrollWidth - host.clientWidth;
            if (overflowY > tolerancePx || overflowX > tolerancePx) {
              out.push({
                testid,
                id: host.getAttribute("data-plan-id") ?? host.getAttribute("data-task-id"),
                overflowY,
                overflowX,
              });
            }
          }
        }
      }
      return out;
    },
    { rootSelector, tolerancePx },
  );
}

export async function findVisibleContentBoundsIssues(
  page: Page,
  rootSelector?: string,
  tolerancePx = 2,
): Promise<ContentBoundsHit[]> {
  return page.evaluate(
    ({ rootSelector, tolerancePx }) => {
      const visible = (el: Element) => {
        const html = el as HTMLElement;
        const rect = html.getBoundingClientRect();
        if (rect.width < 2 || rect.height < 2) return false;
        const cs = getComputedStyle(html);
        return cs.display !== "none" && cs.visibility !== "hidden" && Number(cs.opacity) >= 0.05;
      };
      const within = (a: DOMRect, b: DOMRect) =>
        a.left + tolerancePx >= b.left &&
        a.top + tolerancePx >= b.top &&
        a.right - tolerancePx <= b.right &&
        a.bottom - tolerancePx <= b.bottom;
      const visibleRect = (el: HTMLElement, host: HTMLElement) => {
        const r = el.getBoundingClientRect();
        const out = { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
        for (let p = el.parentElement; p && p !== host; p = p.parentElement) {
          const cs = getComputedStyle(p);
          const clipsX = /(auto|scroll|hidden|clip)/.test(cs.overflowX) && p.scrollWidth > p.clientWidth;
          const clipsY = /(auto|scroll|hidden|clip)/.test(cs.overflowY) && p.scrollHeight > p.clientHeight;
          if (!clipsX && !clipsY) continue;
          const pr = p.getBoundingClientRect();
          out.left = Math.max(out.left, pr.left);
          out.top = Math.max(out.top, pr.top);
          out.right = Math.min(out.right, pr.right);
          out.bottom = Math.min(out.bottom, pr.bottom);
          out.width = Math.max(0, out.right - out.left);
          out.height = Math.max(0, out.bottom - out.top);
        }
        return out;
      };
      const roots = rootSelector
        ? Array.from(document.querySelectorAll(rootSelector))
        : Array.from(document.querySelectorAll('[data-testid="canvas-view"]'));
      const hits: ContentBoundsHit[] = [];
      for (const root of roots) {
        if (!visible(root)) continue;
        for (const node of root.querySelectorAll('[data-testid="canvas-spec-section"], [data-testid="canvas-task-card"]')) {
          if (!visible(node)) continue;
          const host = node as HTMLElement;
          const hostRect = host.getBoundingClientRect();
          const id = host.getAttribute("data-section-id") ?? host.getAttribute("data-task-id");
          for (const child of host.querySelectorAll<HTMLElement>(
            '[data-testid="canvas-spec-section-body"], [data-testid="canvas-task-body"], [data-testid="canvas-task-intent"], [data-testid="canvas-task-why"]',
          )) {
            if (!visible(child)) continue;
            if ((child.textContent ?? "").trim().length === 0) continue;
            const rect = visibleRect(child, host);
            if (rect.height <= 0 || rect.width <= 0) {
              hits.push({ testid: host.dataset.testid ?? "", id, childTestid: child.dataset.testid ?? "", reason: "zero-size-text-block" });
              continue;
            }
            if (!within(rect, hostRect)) {
              hits.push({ testid: host.dataset.testid ?? "", id, childTestid: child.dataset.testid ?? "", reason: "content-outside-node" });
            }
          }
        }
      }
      return hits;
    },
    { rootSelector, tolerancePx },
  );
}

export async function findVisibleOverlaps(page: Page, rootSelector?: string, tolerancePx = 2): Promise<OverlapHit[]> {
  const groups = await Promise.all(
    ["canvas-plan-node", "canvas-spec-node", "canvas-task-card"].map(async (testid) =>
      page.evaluate(
        ({ testid, rootSelector }) => {
          const visible = (el: Element) => {
            const html = el as HTMLElement;
            const rect = html.getBoundingClientRect();
            if (rect.width < 2 || rect.height < 2) return false;
            const cs = getComputedStyle(html);
            return cs.display !== "none" && cs.visibility !== "hidden" && Number(cs.opacity) >= 0.05;
          };
          const roots = rootSelector
            ? Array.from(document.querySelectorAll(rootSelector))
            : Array.from(document.querySelectorAll('[data-testid="canvas-view"]'));
          const out: NodeBox[] = [];
          for (const root of roots) {
            if (!visible(root)) continue;
            for (const el of root.querySelectorAll(`[data-testid="${testid}"]`)) {
              if (!visible(el)) continue;
              const r = el.getBoundingClientRect();
              out.push({
                testid,
                id:
                  el.getAttribute("data-plan-id") ??
                  el.getAttribute("data-task-id") ??
                  el.getAttribute("data-milestone-id") ??
                  el.getAttribute("data-owner-key"),
                box: { x: r.x, y: r.y, width: r.width, height: r.height },
              });
            }
          }
          return out;
        },
        { testid, rootSelector },
      ),
    ),
  );
  const all = groups.flat();
  const hits: OverlapHit[] = [];
  const minArea = tolerancePx * tolerancePx;
  for (let i = 0; i < all.length; i++) {
    for (let j = i + 1; j < all.length; j++) {
      const area = overlapArea(all[i].box, all[j].box);
      if (area > minArea) hits.push({ a: all[i], b: all[j], area });
    }
  }
  return hits;
}

export async function assertVisibleCanvasGeometryClean(page: Page, label: string, rootSelector?: string): Promise<void> {
  const overlaps = await findVisibleOverlaps(page, rootSelector);
  const overflows = await findVisibleOverflows(page, rootSelector);
  const contentBounds = await findVisibleContentBoundsIssues(page, rootSelector);
  expect(overlaps, `${label}: overlapping cards ${JSON.stringify(overlaps.map((h) => [h.a.testid, h.b.testid]))}`).toHaveLength(0);
  expect(overflows, `${label}: clipped cards ${JSON.stringify(overflows)}`).toHaveLength(0);
  expect(contentBounds, `${label}: bad content bounds ${JSON.stringify(contentBounds)}`).toHaveLength(0);
}
