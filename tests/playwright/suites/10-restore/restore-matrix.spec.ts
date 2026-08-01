/**
 * WHAT: Restore-matrix verifies provider tab identity and visible markers after restart.
 * WHY: Session restore must preserve workspace, tab, tab, and agent metadata across hard daemon restarts.
 * ORACLE: Manifest slots match backend tab/tab ids and each expected marker is visible in the restored UI.
 */
import { readFileSync } from "node:fs";
import { expect, test, type ManagedUiSession } from "../../helpers/fixtures";

type MatrixSlot = {
  provider: string;
  index: number;
  requested_model: string;
  booted_model_evidence: string | null;
  tab_id: string | null;
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
const manifest = manifestPath
  ? JSON.parse(readFileSync(manifestPath, "utf8")) as MatrixManifest
  : null;

test.describe.configure({ retries: 0 });

async function visibleChatTextForTab(ui: ManagedUiSession, tabId: string): Promise<string> {
  const chat = ui.page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"]`);
  if ((await chat.count()) === 0) return "";
  return await chat.first().innerText({ timeout: 5_000 }).catch(() => "");
}

function markerNeedle(text: string): string {
  return text.replace(/[^A-Za-z0-9]/g, "").toLowerCase();
}

function isAssertableSlot(slot: MatrixSlot): boolean {
  return !slot.skipped;
}

test("@golden restore matrix markers are isolated in restored provider ChatViews", async ({ api, ui, pollMarkerInDom }) => {
  test.skip(!manifest || !manifestPath, "RESTORE_MATRIX_MANIFEST is required for restore-matrix golden");
  const activeManifest = manifest!;
  test.setTimeout(600_000);
  const asserted = activeManifest.slots.filter(isAssertableSlot);
  expect(asserted.length, `no observable provider tabs in ${manifestPath}`).toBeGreaterThan(0);

  await api.focusWorkspace(activeManifest.workspace.id);
  const wsItem = ui.page.locator(
    `[data-testid="workspace-item"][data-workspace-id="${activeManifest.workspace.id}"]`,
  );
  await expect(wsItem, `workspace ${activeManifest.workspace.name} visible`).toBeVisible({ timeout: 30_000 });
  await wsItem.click();

  const allMarkers = asserted.map((slot) => slot.marker);
  for (const slot of asserted) {
    expect(slot.tab_id, `slot ${slot.provider}/${slot.index} missing tab_id`).toBeTruthy();
    expect(slot.tab_id, `slot ${slot.provider}/${slot.index} missing tab_id`).toBeTruthy();

    await api.focusTab(slot.tab_id!);
    const tab = ui.page.locator(`[data-testid="tab-item"][data-tab-id="${slot.tab_id}"]`);
    await expect(
      tab,
      `tab visible for provider=${slot.provider} slot=${slot.index} tab=${slot.tab_id}`,
    ).toBeVisible({ timeout: 30_000 });
    await tab.click();
    await expect(
      ui.page.locator(`[data-testid="terminal-tab"][data-tab-id="${slot.tab_id}"]`),
      `tab mounted for provider=${slot.provider} slot=${slot.index} tab=${slot.tab_id}`,
    ).toBeVisible({ timeout: 30_000 });
    await api.post(`/tabs/${slot.tab_id!}/chat-view`, { enabled: true });
    const chat = ui.page.locator(`[data-testid="tab-session-chat"][data-tab-id="${slot.tab_id}"]`);
    await expect(
      chat,
      `ChatView visible for provider=${slot.provider} slot=${slot.index} tab=${slot.tab_id}`,
    ).toBeVisible({ timeout: 30_000 });

    try {
      await pollMarkerInDom(slot.marker, {
        tabId: slot.tab_id!,
        timeout: 120_000,
        normalize: true,
        includeChatMessages: true,
      });
    } catch (error) {
      const visibleText = await visibleChatTextForTab(ui, slot.tab_id!);
      throw new Error(
        `ChatView marker timeout provider=${slot.provider} slot=${slot.index} tab=${slot.tab_id} tab=${slot.tab_id} marker=${slot.marker} visibleTail=${JSON.stringify(visibleText.slice(-1000))}: ${String(error)}`,
      );
    }

    const visibleText = await visibleChatTextForTab(ui, slot.tab_id!);
    const normalizedVisible = markerNeedle(visibleText);
    expect(
      normalizedVisible,
      `ChatView marker visible provider=${slot.provider} slot=${slot.index} tab=${slot.tab_id} marker=${slot.marker}`,
    ).toContain(markerNeedle(slot.marker));

    for (const other of allMarkers.filter((m) => m !== slot.marker)) {
      expect(
        normalizedVisible.includes(markerNeedle(other)),
        `ChatView cross-marker contamination provider=${slot.provider} slot=${slot.index} tab=${slot.tab_id} own=${slot.marker} other=${other}`,
      ).toBe(false);
    }
  }
});
