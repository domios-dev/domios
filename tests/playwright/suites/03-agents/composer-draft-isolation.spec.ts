/**
 * WHAT: a freshly-spawned orchestrator/planner pane opens with an EMPTY
 *       composer, and a composer draft never outlives the pane that owns it.
 * WHY: regression guard for the "stray unsubmitted text in a fresh
 *      orchestrator" defect. Root cause: composer drafts live in
 *      localStorage under `codeterm:composer-draft:pane-session-chat:<paneId>`
 *      (see src/lib/usePersistentDraft.ts + ChatComposer.tsx:296), are only
 *      ever cleared on a successful send, and are NEVER reclaimed when a pane
 *      closes. Pane ids are 32-bit `short_id()` and are reusable (claim /
 *      collision), so a new pane whose id matches a stale key inherits a prior
 *      pane's unsent text.
 * ORACLE:
 *   1. INVARIANT (deterministic, the real guard): a draft written for a pane
 *      that is then closed must not survive in localStorage — otherwise a
 *      future pane with the same id inherits it. RED before the fix (nothing
 *      GCs the key), GREEN after.
 *   2. E2E SANITY: a fresh picker pair-spawn's two composers are both empty and
 *      carry no pre-existing draft key.
 *
 * Run EXACTLY (needs a healthy dev on 7686; verify /api/health first):
 *   npx playwright test tests/playwright/suites/03-agents/composer-draft-isolation.spec.ts \
 *     --config=tests/playwright/playwright.config.ts --project=golden \
 *     --timeout=30000 --global-timeout=600000 --reporter=line
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
  test("a draft never outlives the pane that owns it (no stale key to inherit)", async ({
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

  test("a fresh pair-spawn's composers are empty with no pre-existing draft", async ({
    api,
    ui,
  }) => {
    const page = ui.page;
    const ws = await createTestWorkspace(api);
    const before = new Set(
      (await api.workspaces()).find((w) => w.id === ws.id)?.pane_ids ?? [],
    );

    const item = page.locator(`[data-testid="workspace-item"][data-workspace-id="${ws.id}"]`);
    await expect(item).toBeVisible({ timeout: 15_000 });
    const expand = item.locator('button[title="Expand"]');
    if (await expand.isVisible().catch(() => false)) await expand.click();
    await page
      .locator(`[data-testid="workspace-group"][data-workspace-id="${ws.id}"]`)
      .getByText("New session", { exact: true })
      .click();
    await expect(page.locator('[data-testid="new-session-picker"]')).toBeVisible({ timeout: 10_000 });
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();
    await expect(page.locator('[data-testid="new-session-picker"]')).toBeHidden({ timeout: 60_000 });

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

    const keys = await draftKeys(page);
    for (const id of newPaneIds!) {
      const composer = page.locator(`[data-pane-id="${id}"] textarea`).first();
      if (await composer.count()) {
        await expect(composer, `pane ${id} composer is empty`).toHaveValue("");
      }
      expect(keys[`${PANE_CHAT_SCOPE}${id}`] ?? "", `pane ${id} has no stale pane-chat draft`).toBe("");
      expect(keys[`${CHAT_VIEW_SCOPE}${id}`] ?? "", `pane ${id} has no stale chat-view draft`).toBe("");
    }

    await api.closeWorkspace(ws.id).catch(() => {});
  });
});
