/**
 * WHAT: A quiet agent session is readable in the same live status rail a person already watches.
 * WHY: A working-looking tab with a stalled bound session otherwise leaves the person waiting without an actionable explanation.
 * ORACLE: The rendered rail is a polite named status, carries the quiet state hook, and shows the recovery instruction.
 */
import { expect, test, type Page } from "../../helpers/fixtures";

async function mountQuietStatus(page: Page): Promise<void> {
  await page.evaluate(() => {
    const harness = (window as typeof window & {
      __codetermAgentStateHarness?: { mountQuiet: (host: HTMLElement) => unknown };
    }).__codetermAgentStateHarness;
    if (!harness) throw new Error("agent state harness is not installed");
    const host = document.createElement("main");
    document.body.replaceChildren(host);
    harness.mountQuiet(host);
  });
}

test.afterEach(async ({ ui }) => {
  if (ui.page.isClosed()) return;
  await ui.page.reload({ waitUntil: "domcontentloaded" });
  await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
});

test("@golden @wave-1-10-3 quiet session is announced on the agent status rail", async ({ ui }) => {
  await mountQuietStatus(ui.page);
  const status = ui.page.getByRole("status", { name: /agent .*session quiet.*rebind to continue/i });
  await expect(status).toBeVisible();
  await expect(status).toHaveAttribute("aria-live", "polite");
  await expect(status).toHaveAttribute("data-agent-state", "quiet");
  await expect(status).toContainText("Session quiet — rebind to continue");
});
