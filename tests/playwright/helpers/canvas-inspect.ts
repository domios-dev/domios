/**
 * Canvas inspection helper — reusable Playwright primitives for driving and
 * visually auditing the v2 plan-review canvas (workspace + focus modes).
 *
 * macOS/Linux run in web-mode (Chromium against dev /web/); Windows over CDP.
 * Built for a long visual-iteration loop: open the board, pan/zoom/fit/focus,
 * capture full-board + per-region screenshots, and assert no-overlap/no-overflow
 * from real DOM bounding boxes. No app-internal hooks — pure UI driving.
 */
import { expect, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

export const CANVAS_ART_DIR = path.resolve(__dirname, "..", "artifacts", "canvas");

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

export async function openCanvas(page: Page): Promise<void> {
  const view = page.locator('[data-testid="canvas-view"]').first();
  if ((await view.count()) === 0 || !(await view.isVisible().catch(() => false))) {
    // The standalone Canvas nav item was removed; reach a board via the per-pane
    // Plan/Tasks mode segment (workspace overview shows when no tab is focused).
    const planSeg = page
      .locator('[data-testid="pane-mode-switch"] [role="tab"]')
      .filter({ hasText: /Plan|Tasks/ })
      .first();
    if ((await planSeg.count()) > 0) await planSeg.click();
  }
  await view.waitFor({ state: "visible", timeout: 30_000 });
  // Wait for fit-to-view + layout to settle and at least one node to mount.
  await page
    .locator('[data-testid="canvas-plan-node"], [data-testid="canvas-empty"]')
    .first()
    .waitFor({ state: "visible", timeout: 30_000 });
  await page.waitForTimeout(450);
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

// No fit-to-view button; toggle the pane out of and back into Plan mode to remount
// the focus canvas, which re-runs fit-to-view.
export async function fitToView(page: Page): Promise<void> {
  const tabs = page.locator('[data-testid="pane-mode-switch"] [role="tab"]');
  if ((await tabs.count()) < 2) return;
  await tabs.filter({ hasText: /Terminal|Chat/ }).first().click().catch(() => {});
  await page.waitForTimeout(150);
  await tabs.filter({ hasText: /Plan|Tasks/ }).first().click().catch(() => {});
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

// One audit pass: capture + assert invariants. Returns counts for loop logging.
export interface AuditResult {
  nodes: number;
  overlaps: OverlapHit[];
  overflows: OverflowHit[];
  board: string;
}

export async function auditCanvas(page: Page, label: string): Promise<AuditResult> {
  const board = await captureBoard(page, label);
  await captureRegions(page, label);
  const overlaps = await findOverlaps(page);
  const overflows = await findOverflows(page);
  const planNodes = await collectBoxes(page, "canvas-plan-node");
  return { nodes: planNodes.length, overlaps, overflows, board };
}

export function assertClean(result: AuditResult): void {
  expect(result.overlaps, `overlapping cards: ${JSON.stringify(result.overlaps.map((h) => [h.a.testid, h.b.testid, Math.round(h.area)]))}`).toHaveLength(0);
  expect(result.overflows, `clipped/overflowing cards: ${JSON.stringify(result.overflows)}`).toHaveLength(0);
}
