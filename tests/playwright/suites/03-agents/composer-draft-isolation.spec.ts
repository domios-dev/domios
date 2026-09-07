/**
 * WHAT: Fresh tabs never inherit another tab's unsent composer draft.
 * WHY: Tab ids can be reused, so closed-tab draft keys must be reclaimed.
 * ORACLE: Closing clears its key; pair-spawn composers begin empty.
 */
import { expect, test } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";
import type { ApiClient, Workspace } from "../../helpers/api-client";
import type { Page } from "@playwright/test";

const DRAFT_PREFIX = "codeterm:composer-draft:";
const TAB_CHAT_SCOPE = `${DRAFT_PREFIX}tab-session-chat:`;
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
  test("@golden a draft never outlives the tab that owns it (no stale key to inherit)", async ({
    api,
    ui,
  }) => {
    const page = ui.page;
    const ws = await createTestWorkspace(api);

    // Materialize one live tab and seed the exact defect payload as its draft,
    // as if the user had typed it and not sent. Server-minted id → real key.
    await api.createTab({ workspace_id: ws.id });
    const tabId = await pollUntil(
      async () => (await api.workspaces()).find((w) => w.id === ws.id)?.tab_ids?.[0] ?? null,
      15_000,
      500,
      "seed tab materializes",
    );
    const key = `${TAB_CHAT_SCOPE}${tabId}`;
    await page.evaluate(
      ([k, v]) => window.localStorage.setItem(k, v),
      [key, STRAY] as const,
    );
    expect((await draftKeys(page))[key], "seeded draft is present").toBe(STRAY);

    // Close the tab. Its draft must not be left behind for a future
    // (reused/colliding) tab id to inherit.
    await api.closeTab(tabId!);
    await pollUntil(
      async () => (((await api.workspaces()).find((w) => w.id === ws.id)?.tab_ids) ?? []).includes(tabId!) ? null : true,
      15_000,
      500,
      "tab is gone server-side",
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
      (await api.workspaces()).find((w) => w.id === ws.id)?.tab_ids ?? [],
    );

    await page.reload({ waitUntil: "domcontentloaded" });

    // Select the workspace card so its contents region is the user-visible target; the drag wrapper shares its name with the toggle.
    const region = page.getByRole("region", { name: `${ws.name} workspace contents` });
    if (!(await region.isVisible().catch(() => false))) {
      await page
        .getByRole("button", { name: `Select workspace ${ws.name}`, exact: true })
        .and(page.locator("[aria-pressed]"))
        .click();
    }
    await expect(region).toBeVisible({ timeout: 15_000 });

    // Scoping "New session" to this workspace's region keeps a sibling
    // workspace's identically-named control from ever being hit.
    const newSession = region.getByRole("button", { name: "New session" });
    await expect(newSession).toBeVisible({ timeout: 10_000 });
    await newSession.click();

    const picker = page.getByRole("dialog", { name: "Start a session" });
    await expect(picker).toBeVisible({ timeout: 10_000 });
    // Drive the picker by its own testids: the confirm label flips with the
    // mode ("Start team" / "Start session"), so a name match tracks state.
    await picker.locator('[data-testid="picker-mode-orchestration"]').click();
    const confirm = picker.locator('[data-testid="picker-confirm"]');
    await expect(confirm).toBeEnabled({ timeout: 10_000 });
    await confirm.click();
    await expect(picker).toBeHidden({ timeout: 60_000 });

    const newTabIds = await pollUntil(
      async () => {
        const ids = ((await api.workspaces()).find((w) => w.id === ws.id)?.tab_ids ?? []).filter(
          (id) => !before.has(id),
        );
        return ids.length >= 2 ? ids : null;
      },
      60_000,
      1_000,
      "pair tabs materialize",
    );

    // What the user sees: every message composer on screen starts empty.
    const composers = page.getByRole("textbox", { name: "Message composer" });
    for (let i = 0; i < (await composers.count()); i++) {
      await expect(composers.nth(i), `composer #${i} is empty`).toHaveValue("");
    }
    // The defect mechanism: no stale draft key survives for the new tab ids.
    const keys = await draftKeys(page);
    for (const id of newTabIds!) {
      expect(keys[`${TAB_CHAT_SCOPE}${id}`] ?? "", `tab ${id} has no stale tab-chat draft`).toBe("");
      expect(keys[`${CHAT_VIEW_SCOPE}${id}`] ?? "", `tab ${id} has no stale chat-view draft`).toBe("");
    }

    await api.closeWorkspace(ws.id).catch(() => {});
  });
});
