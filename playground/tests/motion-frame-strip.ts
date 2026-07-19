import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { expect, type Locator, type Page, type TestInfo } from "@playwright/test";

export const WORKING_PHASES = [0, 0.1, 0.15, 0.25, 0.5, 0.75, 1] as const;

type Frame = {
  card: string;
  phase: number;
  png: Buffer;
};

function phaseName(phase: number) {
  return `${Math.round(phase * 100)}`.padStart(3, "0");
}

function cardName(card: Locator, index: number) {
  return card.locator("h3").textContent().then((name) =>
    (name ?? `working-${index + 1}`).trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, ""),
  );
}

async function pauseAtPhase(page: Page, phase: number) {
  await page.evaluate(async (nextPhase) => {
    for (const animation of document.getAnimations()) {
      const effect = animation.effect as KeyframeEffect | null;
      effect?.updateTiming({ fill: "both", iterations: 1 });
      animation.pause();
      const duration = effect?.getComputedTiming().duration;
      if (typeof duration === "number" && Number.isFinite(duration)) animation.currentTime = duration * nextPhase;
    }
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  }, phase);
}

async function captureViewport(page: Page, testInfo: TestInfo, run: string, width: number): Promise<Frame[]> {
  await page.setViewportSize({ width, height: 844 });
  await expect.poll(() => page.evaluate(() => document.documentElement.clientWidth)).toBe(width);
  const cards = page.locator(".concept-card[data-working-concept]");
  await expect(cards, "Working concepts must declare data-working-concept on their card.").not.toHaveCount(0);
  const output = testInfo.outputPath("motion-frame-strip", run, `${width}px`);
  await mkdir(output, { recursive: true });
  const frames: Frame[] = [];

  for (const phase of WORKING_PHASES) {
    await pauseAtPhase(page, phase);
    for (let index = 0; index < await cards.count(); index += 1) {
      const card = cards.nth(index);
      const cardId = await cardName(card, index);
      const png = await card.screenshot({ path: join(output, `${cardId}-phase-${phaseName(phase)}.png`) });
      frames.push({ card: cardId, phase, png });
    }
  }
  return frames;
}

export async function captureWorkingFrameStrips(page: Page, testInfo: TestInfo, run: string) {
  const desktop = await captureViewport(page, testInfo, run, 1280);
  const mobile = await captureViewport(page, testInfo, run, 390);
  return { desktop, mobile };
}

export function expectSeamlessWorkingLoops(frames: Frame[]) {
  const byCard = new Map<string, Frame[]>();
  for (const frame of frames) byCard.set(frame.card, [...(byCard.get(frame.card) ?? []), frame]);
  for (const [card, cardFrames] of byCard) {
    const start = cardFrames.find((frame) => frame.phase === 0);
    const end = cardFrames.find((frame) => frame.phase === 1);
    expect(start, `${card} frame strip is missing phase 0.`).toBeDefined();
    expect(end, `${card} frame strip is missing phase 1.`).toBeDefined();
    expect(end!.png, `${card} must pixel-match at loop phases 0 and 1.`).toEqual(start!.png);
  }
}

export function expectIdenticalFrameStrips(first: Frame[], second: Frame[]) {
  expect(second.map(({ card, phase }) => ({ card, phase })), "Repeated frame strips must contain matching frames.")
    .toEqual(first.map(({ card, phase }) => ({ card, phase })));
  for (let index = 0; index < first.length; index += 1) {
    expect(second[index].png, `${first[index].card} phase ${first[index].phase} must be byte-identical across captures.`)
      .toEqual(first[index].png);
  }
}
