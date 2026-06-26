/**
 * Figma-grade canvas acceptance helpers — world vs screen geometry, renderer pixels,
 * and rich DOM overlay budget. Compatible with the current DOM renderer; optional
 * flags gate stricter overlay assertions for future substrate lanes.
 *
 * Focused unit tests: npx tsx tests/playwright/helpers/canvas-figma-acceptance.test.ts
 * Layout fixtures: npx tsx src/components/Canvas/layout/largeCanvasFixtures.test.ts
 */
import { expect, type Page } from "@playwright/test";
import { canvasScale } from "./canvas-inspect";

export const FIGMA_SUBSTRATE_FLAG_KEY = "canvas.substrateV2";
export const PASSIVE_RENDERER_FLAG_KEY = "canvas.passiveRenderer";
export const OVERLAY_BUDGET_FLAG_KEY = "canvas.overlayBudget";
export const DEFAULT_RICH_OVERLAY_BUDGET = 24;

export interface ScreenBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface WorldBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface FootprintSample {
  scale: number;
  screen: ScreenBox;
  world: WorldBox;
}

export interface CanvasPixelSample {
  width: number;
  height: number;
  filledRatio: number;
  nonblank: boolean;
  missing: boolean;
}

export interface RichOverlayBudget {
  enforced: boolean;
  budget: number;
  count: number;
  richCards: string[];
}

export function screenToWorld(screen: Pick<ScreenBox, "w" | "h">, scale: number): Pick<WorldBox, "w" | "h"> {
  if (scale <= 0) return { w: 0, h: 0 };
  return { w: screen.w / scale, h: screen.h / scale };
}

export function worldToScreen(world: Pick<WorldBox, "w" | "h">, scale: number): Pick<ScreenBox, "w" | "h"> {
  return { w: world.w * scale, h: world.h * scale };
}

export function footprintEqual(a: number, b: number): boolean {
  return Math.abs(a - b) <= Math.max(2, 0.05 * Math.max(a, b));
}

export function footprintsStable(samples: Array<Pick<WorldBox, "w" | "h">>): boolean {
  if (samples.length < 2) return samples.length === 1;
  const ref = samples[0];
  return samples.every((s) => footprintEqual(s.w, ref.w) && footprintEqual(s.h, ref.h));
}

export function screenTracksWorldScale(
  world: Pick<WorldBox, "w" | "h">,
  screen: Pick<ScreenBox, "w" | "h">,
  scale: number,
): boolean {
  const expected = worldToScreen(world, scale);
  return footprintEqual(screen.w, expected.w) && footprintEqual(screen.h, expected.h);
}

export async function enableCanvasFeatureFlags(
  page: Page,
  flags: Partial<Record<typeof FIGMA_SUBSTRATE_FLAG_KEY | typeof PASSIVE_RENDERER_FLAG_KEY | typeof OVERLAY_BUDGET_FLAG_KEY, boolean>>,
): Promise<void> {
  await page.addInitScript((entries) => {
    for (const [key, on] of Object.entries(entries)) {
      try {
        if (on) window.localStorage.setItem(key, "true");
        else window.localStorage.removeItem(key);
      } catch {
        // surfaced by callers when reads fail
      }
    }
  }, flags);
}

export async function readScreenBox(
  page: Page,
  testid: string,
  entityId?: string,
  idAttr: "data-task-id" | "data-plan-id" = "data-task-id",
): Promise<ScreenBox | null> {
  return page.evaluate(
    ({ testid, entityId, idAttr }) => {
      const sel = entityId ? `[data-testid="${testid}"][${idAttr}="${entityId}"]` : `[data-testid="${testid}"]`;
      const el = document.querySelector(sel);
      if (!el) return null;
      const r = el.getBoundingClientRect();
      if (r.width < 2 || r.height < 2) return null;
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    },
    { testid, entityId, idAttr },
  );
}

export async function readWorldBox(
  page: Page,
  testid: string,
  entityId?: string,
  idAttr: "data-task-id" | "data-plan-id" = "data-task-id",
): Promise<WorldBox | null> {
  const screen = await readScreenBox(page, testid, entityId, idAttr);
  if (!screen) return null;
  const scale = await canvasScale(page);
  const world = screenToWorld(screen, scale);
  return { x: screen.x / scale, y: screen.y / scale, w: world.w, h: world.h };
}

export async function sampleFootprintsAcrossScales(
  page: Page,
  testid: string,
  scales: number[],
  entityId?: string,
  zoom: (scale: number) => Promise<void> = async () => {},
): Promise<FootprintSample[]> {
  const out: FootprintSample[] = [];
  for (const scale of scales) {
    await zoom(scale);
    const screen = await readScreenBox(page, testid, entityId);
    if (!screen) continue;
    const actualScale = await canvasScale(page);
    const worldSize = screenToWorld(screen, actualScale);
    out.push({
      scale: actualScale,
      screen,
      world: { x: screen.x / actualScale, y: screen.y / actualScale, w: worldSize.w, h: worldSize.h },
    });
  }
  return out;
}

export async function assertWorldFootprintStable(
  page: Page,
  testid: string,
  scales: number[],
  entityId?: string,
  zoom: (scale: number) => Promise<void> = async () => {},
): Promise<FootprintSample[]> {
  const samples = await sampleFootprintsAcrossScales(page, testid, scales, entityId, zoom);
  expect(samples.length, `${testid} footprint samples across scales`).toBeGreaterThanOrEqual(2);
  const worlds = samples.map((s) => s.world);
  expect(footprintsStable(worlds), `world footprint stable: ${JSON.stringify(worlds)}`).toBe(true);
  for (const sample of samples) {
    expect(
      screenTracksWorldScale(sample.world, sample.screen, sample.scale),
      `screen tracks world×scale at ${sample.scale.toFixed(2)}`,
    ).toBe(true);
  }
  return samples;
}

const RENDERER_LAYER_SELECTORS = [
  '[data-testid="canvas-render-layer"]',
  '[data-testid="canvas-renderer-layer"]',
  '[data-testid="canvas-view"] canvas',
  'canvas[data-testid="canvas-passive-layer"]',
] as const;

export async function sampleCanvasPixels(
  page: Page,
  selector = RENDERER_LAYER_SELECTORS.join(", "),
  alphaThreshold = 8,
): Promise<CanvasPixelSample> {
  return page.evaluate(
    ({ selector, alphaThreshold }) => {
      const canvas = document.querySelector(selector) as HTMLCanvasElement | null;
      if (!canvas || canvas.tagName !== "CANVAS") {
        return { width: 0, height: 0, filledRatio: 0, nonblank: false, missing: true };
      }
      const width = canvas.width;
      const height = canvas.height;
      if (width < 1 || height < 1) {
        return { width, height, filledRatio: 0, nonblank: false, missing: false };
      }
      const ctx = canvas.getContext("2d", { willReadFrequently: true });
      if (!ctx) return { width, height, filledRatio: 0, nonblank: false, missing: false };
      const { data } = ctx.getImageData(0, 0, width, height);
      let filled = 0;
      const pixels = width * height;
      for (let i = 3; i < data.length; i += 4) {
        if (data[i] > alphaThreshold) filled += 1;
      }
      const filledRatio = filled / pixels;
      return { width, height, filledRatio, nonblank: filled > 0, missing: false };
    },
    { selector, alphaThreshold },
  );
}

export async function assertCanvasLayerNonblank(
  page: Page,
  minFilledRatio = 0.0005,
  selector?: string,
): Promise<CanvasPixelSample> {
  const sample = await sampleCanvasPixels(page, selector);
  if (sample.missing) return sample;
  expect(sample.nonblank, `canvas layer has drawable pixels (ratio=${sample.filledRatio})`).toBe(true);
  expect(sample.filledRatio, "canvas filled ratio above floor").toBeGreaterThanOrEqual(minFilledRatio);
  return sample;
}

export async function readRichOverlayBudget(page: Page, budget = DEFAULT_RICH_OVERLAY_BUDGET): Promise<RichOverlayBudget> {
  return page.evaluate(
    ({ budget, flagKey }) => {
      const enforced = (() => {
        try {
          return window.localStorage.getItem(flagKey) === "true";
        } catch {
          return false;
        }
      })();
      const richCards = [...document.querySelectorAll('[data-testid="canvas-task-card"], [data-testid="canvas-spec-node"]')]
        .filter((card) => {
          const html = card as HTMLElement;
          const cs = getComputedStyle(html);
          const rect = html.getBoundingClientRect();
          if (rect.width < 2 || rect.height < 2) return false;
          if (cs.display === "none" || cs.visibility === "hidden" || Number(cs.opacity) < 0.05) return false;
          const rich = card.querySelector(
            '[data-testid="canvas-task-full-body"], [data-testid="canvas-task-body"], [data-testid="canvas-spec-section-body"]',
          ) as HTMLElement | null;
          if (!rich) return false;
          const richCs = getComputedStyle(rich);
          if (richCs.display === "none" || richCs.visibility === "hidden" || Number(richCs.opacity) < 0.05) return false;
          const richRect = rich.getBoundingClientRect();
          return richRect.width > 2 && richRect.height > 2 && (rich.textContent ?? "").trim().length > 0;
        })
        .map((card) => card.getAttribute("data-task-id") ?? card.getAttribute("data-plan-id") ?? card.getAttribute("data-testid") ?? "unknown");
      return { enforced, budget, count: richCards.length, richCards };
    },
    { budget, flagKey: OVERLAY_BUDGET_FLAG_KEY },
  );
}

export async function assertRichOverlayBudget(page: Page, max = DEFAULT_RICH_OVERLAY_BUDGET): Promise<RichOverlayBudget> {
  const snapshot = await readRichOverlayBudget(page, max);
  if (!snapshot.enforced) return snapshot;
  expect(
    snapshot.count,
    `rich DOM overlay budget (${max}) — mounted: ${snapshot.richCards.join(", ")}`,
  ).toBeLessThanOrEqual(max);
  return snapshot;
}

export async function assertFocusTargetMounted(page: Page, testid: string, entityId: string): Promise<ScreenBox> {
  const box = await readScreenBox(page, testid, entityId);
  expect(box, `focus target ${testid}#${entityId} mounted on screen`).toBeTruthy();
  return box!;
}
