// Review-gate conformance proves Motion Lab tiers, semantic stages, tokenized motion, and paused-frame pixels.
// Working frame strips are emitted below Playwright's per-run output path.
import { expect, test, type Page } from "@playwright/test";
import { captureWorkingFrameStrips, expectIdenticalFrameStrips, expectSeamlessWorkingLoops } from "./motion-frame-strip";

const MOTION_LAB_URL = process.env.MOTION_LAB_URL ?? "http://127.0.0.1:1431";

async function openMotionLab(page: Page) {
  await page.goto(MOTION_LAB_URL, { waitUntil: "domcontentloaded" });
  await expect(page.getByRole("heading", { name: "Motion Lab" })).toBeVisible();
}

function tierCount(page: Page, headingId: string) {
  return page.locator(`section[aria-labelledby="${headingId}"] .gallery-tier-count`);
}

async function tokenDurationViolations(page: Page) {
  return page.evaluate(() => {
    const parseDuration = (value: string) => {
      const match = value.trim().match(/^([0-9.]+)(ms|s)$/);
      if (!match) return Number.NaN;
      return Number(match[1]) * (match[2] === "s" ? 1000 : 1);
    };
    const root = document.querySelector(".motion-lab") as HTMLElement;
    const tokens = getComputedStyle(root);
    const allowed = [
      "--motion-duration-instant",
      "--motion-duration-fast",
      "--motion-duration-moderate",
      "--motion-duration-expressive",
      "--motion-period-ambient",
    ].map((name) => parseDuration(tokens.getPropertyValue(name)));
    const inScope = (element: Element) => {
      const card = element.closest(".concept-card");
      if (!card) return false;
      return !!card.closest(".accepted-tier") || !!card.closest(".exploration-tier") && card.hasAttribute("data-motion-lane");
    };
    const describe = (element: Element) => {
      const card = element.closest(".concept-card");
      const title = card?.querySelector("h3")?.textContent?.trim() ?? "unnamed concept";
      return `${title} :: ${element.tagName.toLowerCase()}${element.className ? `.${String(element.className).trim().replace(/\\s+/g, ".")}` : ""}`;
    };
    return document.getAnimations().flatMap((animation) => {
      const target = (animation.effect as KeyframeEffect | null)?.target;
      if (!(target instanceof Element) || !inScope(target)) return [];
      const durations = getComputedStyle(target).animationDuration.split(",").map(parseDuration);
      return durations
        .filter((duration) => !allowed.some((token) => Math.abs(duration - token) < 0.01))
        .map((duration) => ({ element: describe(target), duration, allowed }));
    });
  });
}

test("review gate exposes current Accepted and Exploration counts", async ({ page }) => {
  await openMotionLab(page);
  await expect(tierCount(page, "accepted-concepts-title")).toHaveText("0 concepts");
  await expect(tierCount(page, "exploration-concepts-title")).toHaveText("16 concepts");
});

test("review gate removes every ambient animation under reduced motion", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openMotionLab(page);
  const durations = await page.evaluate(() => Array.from(document.querySelectorAll("*")).flatMap((element) => {
    const style = getComputedStyle(element);
    return style.animationName === "none" ? [] : [{
      element: `${element.tagName.toLowerCase()}.${element.className}`,
      duration: style.animationDuration,
    }];
  }));
  expect(durations.every(({ duration }) => duration.split(",").every((value) => value.trim() === "0s")), "Reduced motion must resolve every ambient animation to 0s.").toBe(true);
});

test("review gate keeps each concept stage named and mobile width overflow-free", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openMotionLab(page);
  const stages = page.locator(".concept-stage");
  await expect(stages).not.toHaveCount(0);
  for (let index = 0; index < await stages.count(); index += 1) {
    const stage = stages.nth(index);
    await expect(stage).toHaveAttribute("role", "img");
    await expect(stage).toHaveAccessibleName(/\S/);
  }
  await expect.poll(() => page.evaluate(() => ({ client: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth })))
    .toEqual({ client: 390, scroll: 390 });
});

test("review gate rejects off-token animation durations in accepted and lane-tagged concepts", async ({ page }) => {
  await openMotionLab(page);
  await expect.poll(() => tokenDurationViolations(page)).toEqual([]);
});

test("review gate captures deterministic Working frame strips with seamless loop endpoints", async ({ page }, testInfo) => {
  await openMotionLab(page);
  const first = await captureWorkingFrameStrips(page, testInfo, "first");
  expectSeamlessWorkingLoops(first.desktop);
  expectSeamlessWorkingLoops(first.mobile);
  await page.reload({ waitUntil: "domcontentloaded" });
  await expect(page.getByRole("heading", { name: "Motion Lab" })).toBeVisible();
  const second = await captureWorkingFrameStrips(page, testInfo, "second");
  expectIdenticalFrameStrips(first.desktop, second.desktop);
  expectIdenticalFrameStrips(first.mobile, second.mobile);
});
