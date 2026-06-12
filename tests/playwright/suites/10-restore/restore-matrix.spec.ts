import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { ApiClient } from "../../helpers/api-client";
import { connectUi, type UiSession } from "../../helpers/dev-console";

type MatrixSlot = {
  provider: string;
  index: number;
  requested_model: string;
  booted_model_evidence: string | null;
  pane_id: string | null;
  tab_id: string | null;
  marker: string;
  sid_before: string | null;
  sid_after: string | null;
  skipped: boolean;
  skip_reason: string | null;
};

type MatrixManifest = {
  run_id: string;
  workspace: { id: string; name: string };
  slots: MatrixSlot[];
};

const manifestPath = process.env.RESTORE_MATRIX_MANIFEST;
if (!manifestPath) {
  throw new Error("RESTORE_MATRIX_MANIFEST is required");
}
const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as MatrixManifest;
const api = new ApiClient();
let ui: UiSession;

test.beforeAll(async () => {
  ui = await connectUi();
});

test.afterAll(async () => {
  await ui.browser.close();
});

async function visibleTerminalTextForPane(paneId: string): Promise<string> {
  return ui.page.evaluate((id) => {
    const pane = document.querySelector(`[data-pane-id="${id}"]`);
    const roots = pane
      ? Array.from(pane.querySelectorAll(".xterm-rows, .xterm-accessibility-tree"))
      : Array.from(document.querySelectorAll(".xterm-rows, .xterm-accessibility-tree"));
    return roots
      .filter((el) => el.tagName !== "STYLE" && el.tagName !== "SCRIPT")
      .map((el) => (el as HTMLElement).innerText || el.textContent || "")
      .join("\n");
  }, paneId);
}

function markerNeedle(text: string): string {
  return text.replace(/[^A-Za-z0-9]/g, "").toLowerCase();
}

function isAssertableSlot(slot: MatrixSlot): boolean {
  return !slot.skipped;
}

test("@golden restore matrix markers are visible in their restored provider panes", async () => {
  test.setTimeout(600_000);
  const asserted = manifest.slots.filter(isAssertableSlot);
  expect(asserted.length, `no observable provider panes in ${manifestPath}`).toBeGreaterThan(0);

  await api.focusWorkspace(manifest.workspace.id);
  const wsItem = ui.page.locator(
    `[data-testid="workspace-item"][data-workspace-id="${manifest.workspace.id}"]`,
  );
  await expect(wsItem, `workspace ${manifest.workspace.name} visible`).toBeVisible({ timeout: 30_000 });
  await wsItem.click();

  const allMarkers = asserted.map((slot) => slot.marker);
  for (const slot of asserted) {
    expect(slot.pane_id, `slot ${slot.provider}/${slot.index} missing pane_id`).toBeTruthy();
    expect(slot.tab_id, `slot ${slot.provider}/${slot.index} missing tab_id`).toBeTruthy();

    await api.focusPane(slot.pane_id!);
    const tab = ui.page.locator(`[data-testid="tab-item"][data-tab-id="${slot.tab_id}"]`);
    await expect(
      tab,
      `tab visible for provider=${slot.provider} slot=${slot.index} pane=${slot.pane_id}`,
    ).toBeVisible({ timeout: 30_000 });
    await tab.click();

    try {
      await ui.page.waitForFunction(
        ({ paneId, marker }) => {
          const normalize = (text: string) => text.replace(/[^A-Za-z0-9]/g, "").toLowerCase();
          const needle = normalize(marker);
          const pane = document.querySelector(`[data-pane-id="${paneId}"]`);
          const roots = pane
            ? Array.from(pane.querySelectorAll(".xterm-rows, .xterm-accessibility-tree"))
            : Array.from(document.querySelectorAll(".xterm-rows, .xterm-accessibility-tree"));
          return roots
            .filter((el) => el.tagName !== "STYLE" && el.tagName !== "SCRIPT")
            .some((el) => normalize((el as HTMLElement).innerText || el.textContent || "").includes(needle));
        },
        { paneId: slot.pane_id!, marker: slot.marker },
        { timeout: 120_000, polling: 1_000 },
      );
    } catch (error) {
      const visibleText = await visibleTerminalTextForPane(slot.pane_id!);
      throw new Error(
        `marker timeout provider=${slot.provider} slot=${slot.index} pane=${slot.pane_id} tab=${slot.tab_id} marker=${slot.marker} visibleTail=${JSON.stringify(visibleText.slice(-1000))}: ${String(error)}`,
      );
    }

    const visibleText = await visibleTerminalTextForPane(slot.pane_id!);
    const normalizedVisible = markerNeedle(visibleText);
    expect(
      normalizedVisible,
      `marker visible provider=${slot.provider} slot=${slot.index} pane=${slot.pane_id} marker=${slot.marker}`,
    ).toContain(markerNeedle(slot.marker));

    for (const other of allMarkers.filter((m) => m !== slot.marker)) {
      expect(
        normalizedVisible.includes(markerNeedle(other)),
        `cross-marker contamination provider=${slot.provider} slot=${slot.index} pane=${slot.pane_id} own=${slot.marker} other=${other}`,
      ).toBe(false);
    }
  }
});
