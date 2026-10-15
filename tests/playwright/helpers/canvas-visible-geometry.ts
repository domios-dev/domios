import { expect, type Page } from "@playwright/test";
import type { ContentBoundsHit, NodeBox, OverflowHit, OverlapHit } from "./canvas-inspect";

function overlapArea(a: NodeBox["box"], b: NodeBox["box"]): number {
  const x = Math.max(0, Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x));
  const y = Math.max(0, Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y));
  return x * y;
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
      const within = (a: Pick<DOMRect, "left" | "top" | "right" | "bottom">, b: Pick<DOMRect, "left" | "top" | "right" | "bottom">) =>
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
