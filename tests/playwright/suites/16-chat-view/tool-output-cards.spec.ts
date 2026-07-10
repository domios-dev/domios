/**
 * WHAT: Shared tool-output cards retain complete transport envelopes behind a bounded preview.
 * WHY: Tool output must stay usable in the short Agent Run log without silently losing data.
 * ORACLE: Desktop and phone cards never overflow, and expansion/copy reveal the exact payload.
 */
import { expect, test, type Page } from "../../helpers/fixtures";

const DESKTOP = { width: 959, height: 279 };
const MOBILE = { width: 320, height: 568 };
const LONG_JSON = JSON.stringify({ envelope: "x".repeat(4_096), status: "complete" });

declare global {
  interface Window {
    __toolOutputCardTest?: { copied: string; root?: { unmount: () => void } };
  }
}

async function mountCards(page: Page): Promise<void> {
  await page.evaluate(async (payload) => {
    const React = await import("react");
    const { createRoot } = await import("react-dom/client");
    const { ToolOutputCard } = await import("/src/components/AgentRun/ToolOutputCard.tsx");
    const host = document.createElement("main");
    host.style.cssText = "box-sizing:border-box;max-width:100vw;overflow:hidden;padding:8px";
    document.body.replaceChildren(host);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async (value: string) => { window.__toolOutputCardTest!.copied = value; } },
    });
    window.__toolOutputCardTest = { copied: "" };
    const root = createRoot(host);
    window.__toolOutputCardTest.root = root;
    root.render(
      React.createElement(
        "div",
        { style: { minWidth: 0, maxWidth: "100%" } },
        React.createElement(ToolOutputCard, { kind: "call", toolName: "exec", payload }),
        React.createElement(ToolOutputCard, { kind: "call", toolName: "wait", payload: "one line" }),
        React.createElement(ToolOutputCard, { kind: "call", toolName: "ssh", payload: "ssh success" }),
        React.createElement(ToolOutputCard, { kind: "call", toolName: "mesh", payload: "mesh timeout" }),
        React.createElement(ToolOutputCard, { kind: "call", toolName: "plugin", payload: "plugin result" }),
        React.createElement(ToolOutputCard, { kind: "result", payload: "success", isError: false }),
        React.createElement(ToolOutputCard, { kind: "result", payload: "failure", isError: true }),
        React.createElement(ToolOutputCard, { kind: "result", payload: "timeout", isError: true }),
      ),
    );
  }, LONG_JSON);
  await expect(page.locator('[data-testid="tool-output-card"]').first()).toBeVisible();
}

async function expectBoundedPreview(page: Page): Promise<void> {
  const card = page.locator('[data-testid="tool-output-card"][data-tool-name="exec"]');
  const preview = card.locator('[data-testid="tool-output-preview"]');
  await expect(card.getByRole("button", { name: /expand full exec call/i })).toBeVisible();
  await expect(preview).toHaveCSS("overflow", "hidden");
  await expect(preview).toHaveCSS("overflow-wrap", "anywhere");
  await expect(preview).toHaveCSS("-webkit-line-clamp", "2");
  await expect(card).toContainText("…");
  await expect(page.locator("body")).toEvaluate((body) => body.scrollWidth <= body.clientWidth);
  await expect(card).toEvaluate((element) => element.scrollWidth <= element.clientWidth);
}

test.afterEach(async ({ ui }) => {
  await ui.page.evaluate(() => window.__toolOutputCardTest?.root?.unmount());
});

test("@golden shared tool cards bound long exec output at 959x279 and mobile", async ({ ui }) => {
  await ui.page.setViewportSize(DESKTOP);
  await mountCards(ui.page);
  await expectBoundedPreview(ui.page);
  await ui.page.screenshot({ path: test.info().outputPath("tool-output-cards-959x279.png") });

  await ui.page.setViewportSize(MOBILE);
  await expectBoundedPreview(ui.page);
  await ui.page.screenshot({ path: test.info().outputPath("tool-output-cards-mobile.png") });
});

test("@golden shared tool cards expand by pointer and keyboard, then copy the complete payload", async ({ ui }) => {
  await ui.page.setViewportSize(DESKTOP);
  await mountCards(ui.page);
  const card = ui.page.locator('[data-testid="tool-output-card"][data-tool-name="exec"]');
  const expand = card.getByRole("button", { name: /expand full exec call/i });

  await expand.click();
  const full = card.locator('[data-testid="tool-output-full"]');
  await expect(full).toHaveText(LONG_JSON);
  await expect(full).toHaveCSS("overflow", "auto");
  await expect(full).toEvaluate((element) => element.scrollHeight >= element.clientHeight);
  await card.getByRole("button", { name: /collapse exec call/i }).click();

  await expand.focus();
  await ui.page.keyboard.press("Enter");
  await expect(card.getByRole("button", { name: /collapse exec call/i })).toHaveAttribute("aria-expanded", "true");
  await card.getByRole("button", { name: "Copy full output" }).click();
  await expect.poll(() => ui.page.evaluate(() => window.__toolOutputCardTest?.copied)).toBe(LONG_JSON);
  await ui.page.keyboard.press("Tab");
  await ui.page.keyboard.press("Enter");
  await expect(card.getByRole("button", { name: /expand full exec call/i })).toHaveAttribute("aria-expanded", "false");
});

test("@golden shared tool cards preserve success, failure, timeout, and representative transports", async ({ ui }) => {
  await mountCards(ui.page);
  for (const name of ["exec", "wait", "ssh", "mesh", "plugin"]) {
    await expect(ui.page.locator(`[data-testid="tool-output-card"][data-tool-name="${name}"]`)).toBeVisible();
  }
  const results = ui.page.locator('[data-testid="tool-output-card"][data-tool-kind="result"]');
  await expect(results).toHaveCount(3);
  await expect(results.nth(0).locator('[data-testid="tool-output-status"]')).toHaveText("complete");
  await expect(results.nth(1).locator('[data-testid="tool-output-status"]')).toHaveText("failed");
  await expect(results.nth(2).locator('[data-testid="tool-output-status"]')).toHaveText("failed");
});
