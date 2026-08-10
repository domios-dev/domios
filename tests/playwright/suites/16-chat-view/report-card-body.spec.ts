/**
 * WHAT: Report cards render the agent's free-form message alongside the structured status chip.
 * WHY: Structured fields must add to an AgentReport's body, never replace it, on every transport.
 * ORACLE: The real Curie status-plus-multiline payload shows its status chip AND body lines, local and mesh-stamped.
 */
import { expect, test, type Page } from "../../helpers/fixtures";
import { CURIE_PARTIAL_REPORT_BODY_MARKERS } from "../../../../packages/chat-ui/src/codeterm/inboundReportFixtures.testdata";

const mountedPages = new Set<Page>();

declare global {
  interface Window {
    __inboundReportTest?: { root?: { unmount: () => void } };
    __codetermInboundReportHarness?: { mount: (host: HTMLElement) => { unmount: () => void } };
  }
}

async function mountReportCards(page: Page): Promise<void> {
  await page.evaluate(() => {
    const harness = window.__codetermInboundReportHarness;
    if (!harness) throw new Error("inbound report card harness is not installed");
    const host = document.createElement("main");
    host.style.cssText = "box-sizing:border-box;max-width:100vw;padding:8px";
    document.body.replaceChildren(host);
    window.__inboundReportTest = { root: harness.mount(host) };
  });
  mountedPages.add(page);
  await expect(page.locator('[data-testid="chat-inbound-report"]')).toHaveCount(2);
}

test.afterEach(async () => {
  const pages = Array.from(mountedPages);
  mountedPages.clear();
  for (const page of pages) {
    if (page.isClosed()) continue;
    await page.evaluate(() => window.__inboundReportTest?.root?.unmount());
  }
});

test("@golden report card body renders with the status chip for local and mesh reports", async ({ ui }) => {
  await mountReportCards(ui.page);

  for (const variant of ["local", "mesh"]) {
    const card = ui.page.locator(`[data-testid="report-variant-${variant}"] [data-testid="chat-inbound-report"]`);
    await expect(card).toBeVisible();
    await expect(card).toContainText("Partial");

    // The card lands collapsed; the body is behind its own disclosure.
    const body = card.locator('[data-testid="chat-inbound-report-body"]');
    await expect(body).toHaveCount(0);
    await card.getByRole("button", { name: "Show report Details" }).click();

    await expect(body).toBeVisible();
    for (const line of CURIE_PARTIAL_REPORT_BODY_MARKERS) {
      await expect(body).toContainText(line);
    }
    // The body is the message, not a status echo: it must outweigh everything
    // else the card renders (chip, header, structured fields).
    const cardLength = await card.evaluate((element) => (element.textContent ?? "").length);
    const bodyLength = await body.evaluate((element) => (element.textContent ?? "").length);
    expect(bodyLength, `${variant} report body must outweigh its structured chrome`)
      .toBeGreaterThan(cardLength - bodyLength);
  }
});
