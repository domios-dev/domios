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
    await page.evaluate(({ currentScale, nextScale }) => {
      const view = document.querySelector('[data-testid="canvas-view"]');
      const target = view?.querySelector("div");
      if (!target) return;
      const r = target.getBoundingClientRect();
      const deltaY = -Math.log(nextScale / currentScale) / 0.0016;
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
    }, { currentScale: current, nextScale: targetScale });
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
          const rect = child.getBoundingClientRect();
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
  return page.$$eval(
    '[data-testid="canvas-plan-node"], [data-testid="canvas-spec-section"], [data-testid="canvas-task-card"]',
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
          '[data-testid="canvas-plan-row"], [data-testid="canvas-spec-section"], [data-testid="canvas-task-card"], [data-testid="canvas-milestone-bubble"]',
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
