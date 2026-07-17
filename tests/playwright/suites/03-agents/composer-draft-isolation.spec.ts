/**
 * WHAT: Fresh panes never inherit another pane's unsent composer draft.
 * WHY: Pane ids can be reused, so closed-pane draft keys must be reclaimed.
 * ORACLE: Closing clears its key; pair-spawn composers begin empty.
 */
import { expect, test } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";
import type { ApiClient, Workspace } from "../../helpers/api-client";
import type { Page } from "@playwright/test";

const DRAFT_PREFIX = "codeterm:composer-draft:";
const PANE_CHAT_SCOPE = `${DRAFT_PREFIX}pane-session-chat:`;
const CHAT_VIEW_SCOPE = `${DRAFT_PREFIX}chat-view:`;
const STRAY = "continue working on the 1.7.2 stability plan";

async function draftKeys(page: Page): Promise<Record<string, string>> {
  return page.evaluate((prefix) => {
    const out: Record<string, string> = {};
    for (let i = 0; i < window.localStorage.length; i++) {
      const k = window.localStorage.key(i);
      if (k && k.startsWith(prefix)) out[k] = window.localStorage.getItem(k) ?? "";
    }
    return out;
  }, DRAFT_PREFIX);
}

async function createTestWorkspace(api: ApiClient): Promise<Workspace> {
  const name = `pw-draft-iso-${Date.now()}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find((w) => w.name === name);
  if (!ws) throw new Error(`workspace ${name} missing after create`);
  await api.focusWorkspace(ws.id);
  return ws;
}

test.describe("@golden composer draft isolation", () => {
  test("@golden a draft never outlives the pane that owns it (no stale key to inherit)", async ({
    api,
    ui,
  }) => {
    const page = ui.page;
    const ws = await createTestWorkspace(api);

    // Materialize one live pane and seed the exact defect payload as its draft,
    // as if the user had typed it and not sent. Server-minted id → real key.
    await api.createPane({ workspace_id: ws.id });
    const paneId = await pollUntil(
      async () => (await api.workspaces()).find((w) => w.id === ws.id)?.pane_ids?.[0] ?? null,
      15_000,
      500,
      "seed pane materializes",
    );
    const key = `${PANE_CHAT_SCOPE}${paneId}`;
    await page.evaluate(
      ([k, v]) => window.localStorage.setItem(k, v),
      [key, STRAY] as const,
    );
    expect((await draftKeys(page))[key], "seeded draft is present").toBe(STRAY);

    // Close the pane. Its draft must not be left behind for a future
    // (reused/colliding) pane id to inherit.
    await api.closePane(paneId!);
    await pollUntil(
      async () => (((await api.workspaces()).find((w) => w.id === ws.id)?.pane_ids) ?? []).includes(paneId!) ? null : true,
      15_000,
      500,
      "pane is gone server-side",
    );

    await expect
      .poll(async () => (await draftKeys(page))[key] ?? null, { timeout: 10_000 })
      .toBeNull();

    await api.closeWorkspace(ws.id).catch(() => {});
  });

  test("@golden a fresh pair-spawn's composers are empty with no pre-existing draft", async ({
    api,
    ui,
  }) => {
    const page = ui.page;
    const ws = await createTestWorkspace(api);
    const before = new Set(
      (await api.workspaces()).find((w) => w.id === ws.id)?.pane_ids ?? [],
    );

    // The user journey, driven the way a person (or screen reader) would:
    // open the workspace, start a new session, pick Orchestration, start it.
    const workspaceRow = page.getByRole("button", { name: `Open workspace ${ws.name}`, exact: true });
    await expect(workspaceRow).toBeVisible({ timeout: 15_000 });
    const expand = page.getByRole("button", { name: "Expand", expanded: false });
    if (await expand.first().isVisible().catch(() => false)) await expand.first().click();
    await page.getByRole("button", { name: "New session" }).first().click();

    const picker = page.getByRole("dialog", { name: "Start a session" });
    await expect(picker).toBeVisible({ timeout: 10_000 });
    await picker.getByRole("button", { name: /Orchestrat/ }).first().click();
    await picker.getByRole("button", { name: "Start orchestration" }).click();
    await expect(picker).toBeHidden({ timeout: 60_000 });

    const newPaneIds = await pollUntil(
      async () => {
        const ids = ((await api.workspaces()).find((w) => w.id === ws.id)?.pane_ids ?? []).filter(
          (id) => !before.has(id),
        );
        return ids.length >= 2 ? ids : null;
      },
      60_000,
      1_000,
      "pair panes materialize",
    );

    // What the user sees: every message composer on screen starts empty.
    const composers = page.getByRole("textbox", { name: "Message composer" });
    for (let i = 0; i < (await composers.count()); i++) {
      await expect(composers.nth(i), `composer #${i} is empty`).toHaveValue("");
    }
    // The defect mechanism: no stale draft key survives for the new pane ids.
    const keys = await draftKeys(page);
    for (const id of newPaneIds!) {
      expect(keys[`${PANE_CHAT_SCOPE}${id}`] ?? "", `pane ${id} has no stale pane-chat draft`).toBe("");
      expect(keys[`${CHAT_VIEW_SCOPE}${id}`] ?? "", `pane ${id} has no stale chat-view draft`).toBe("");
    }

    await api.closeWorkspace(ws.id).catch(() => {});
  });
});
