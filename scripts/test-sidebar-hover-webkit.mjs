import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { webkit } from "@playwright/test";

const baseUrl = process.env.SIDEBAR_HOVER_URL ?? "http://127.0.0.1:1427/tests/fixtures/sidebar-hover/index.html";
const screenshotDir = process.env.SIDEBAR_HOVER_SCREENSHOT_DIR ?? path.join(tmpdir(), "codeterm-sidebar-hover-webkit");
await mkdir(screenshotDir, { recursive: true });

const browser = await webkit.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 720, height: 920 }, deviceScaleFactor: 1 });
const pageErrors = [];
page.on("pageerror", (error) => pageErrors.push(error.message));

async function settleTransformTransitions() {
  await page.waitForFunction(() => document.getAnimations().every((animation) =>
    animation.playState !== "running"
      || !("transitionProperty" in animation)
      || animation.transitionProperty !== "transform"));
}

function differs(before, after) {
  return ["x", "y", "width", "height"].some((key) => Math.abs(before[key] - after[key]) > 0.25);
}

async function readBoxes(surfaces) {
  return surfaces.evaluateAll((elements) => elements.map((element) => {
    const rect = element.getBoundingClientRect();
    return { label: element.getAttribute("aria-label") ?? element.getAttribute("data-row-id") ?? element.textContent?.trim() ?? "row", x: rect.x, y: rect.y, width: rect.width, height: rect.height };
  }));
}

try {
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.getByTestId("sidebar-hover-fixture").waitFor();
  await page.mouse.move(700, 880);
  await settleTransformTransitions();
  const surfaces = page.locator("[data-sidebar-hover-row]");
  const before = await readBoxes(surfaces);
  await page.screenshot({ path: path.join(screenshotDir, "webkit-before.png"), fullPage: true });

  const geometryFailures = [];
  for (let index = 0; index < before.length; index += 1) {
    const target = surfaces.nth(index);
    const box = await target.boundingBox();
    assert.ok(box, `hover surface ${before[index].label} has a visible bounding box`);
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await settleTransformTransitions();
    const after = await readBoxes(surfaces);
    for (let surfaceIndex = 0; surfaceIndex < before.length; surfaceIndex += 1) {
      if (differs(before[surfaceIndex], after[surfaceIndex])) {
        geometryFailures.push(`hover ${before[index].label} changes ${before[surfaceIndex].label}: before=${JSON.stringify(before[surfaceIndex])}, after=${JSON.stringify(after[surfaceIndex])}`);
      }
    }
  }

  await page.screenshot({ path: path.join(screenshotDir, "webkit-after-hover.png"), fullPage: true });
  await page.mouse.move(700, 880);
  await settleTransformTransitions();
  const overlappingRows = await page.locator("[data-sidebar-layout-row='true']").evaluateAll((elements) => {
    const boxes = elements.map((element) => ({
      label: element.getAttribute("aria-label") ?? element.textContent?.trim() ?? "row",
      rect: element.getBoundingClientRect(),
    })).sort((left, right) => left.rect.top - right.rect.top);
    return boxes.slice(1).flatMap((current, index) => {
      const previous = boxes[index];
      return previous.rect.bottom > current.rect.top + 0.25
        ? [`${previous.label} overlaps ${current.label}`]
        : [];
    });
  });

  const generalRows = page.locator('[data-dnd-group="tab:general"]');
  const from = await generalRows.nth(0).boundingBox();
  const to = await generalRows.nth(1).boundingBox();
  assert.ok(from && to, "sortable rows have visible bounding boxes");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 8 });
  await page.waitForFunction(() => document.querySelector('[data-dnd-dragging="true"]') !== null);
  await page.mouse.up();
  await page.waitForFunction(() => Array.from(document.querySelectorAll(".fixture-general-rows [data-row-id]"))
    .map((row) => row.getAttribute("data-row-id")).join(",") === "rubin,turing,sagan");
  await page.waitForFunction(() => Array.from(document.querySelectorAll('[data-dnd-group="tab:general"]'))
    .every((row) => getComputedStyle(row).transform === "none"));

  const residualTransforms = await page.locator("[data-sidebar-layout-row='true'], .sidebar-tab, .sidebar-tab-activation, .sidebar-workspace-card-select, .sidebar-workspace-card-create, [data-dnd-group]")
    .evaluateAll((elements) => elements.flatMap((element) => {
      const transform = getComputedStyle(element).transform;
      return transform === "none" ? [] : [`${element.className || element.tagName}: ${transform}`];
    }));
  await page.evaluate(() => (document.activeElement instanceof HTMLElement) && document.activeElement.blur());
  await page.screenshot({ path: path.join(screenshotDir, "webkit-after-layout-transition.png"), fullPage: true });

  assert.deepEqual(pageErrors, [], "fixture renders without browser errors");
  assert.deepEqual(geometryFailures, [], "hovering sidebar rows preserves every bounding box");
  assert.deepEqual(overlappingRows, [], "sidebar layout rows do not overlap");
  assert.deepEqual(residualTransforms, [], "hover and sortable layout transitions leave no residual row transforms");
  console.log(`PASS WebKit: ${before.length} hover surfaces kept their bounding boxes; 3 sortable rows reordered and settled; screenshots saved to ${screenshotDir}`);
} finally {
  await browser.close();
}
