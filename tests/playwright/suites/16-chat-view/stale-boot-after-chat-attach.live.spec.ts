/**
 * WHAT: Attached Codex Chat remains authoritative after stale boot-stage events.
 * WHY: Late spawn failures must not cover a live conversation or interrupt its composer.
 * ORACLE: Chat stays visible through spawn_failed and pty_reattached events.
 */
import type { Page } from "@playwright/test";
import { expect, test } from "../../helpers/fixtures";

type SpawnedTab = { tab_id: string; agent: string };
type MuxState = {
  workspaces?: Array<{ id: string; tabs?: Array<{ id: string; tab_id: string }> }>;
};

async function activateTabChat(
  api: { get: <T>(path: string) => Promise<T>; post: (path: string, body?: unknown) => Promise<unknown> },
  tabId: string,
): Promise<void> {
  const mux = await api.get<MuxState>("/mux/state");
  for (const workspace of mux.workspaces ?? []) {
    const tab = workspace.tabs?.find((candidate) => candidate.tab_id === tabId);
    if (!tab) continue;
    await api.post(`/tabs/${tabId}/chat-view`, { enabled: true });
    await api.post(`/workspaces/${workspace.id}/focus`);
    await api.post(`/workspaces/${workspace.id}/tabs/${tab.id}/focus`);
    await api.post(`/tabs/${tabId}/focus`);
    return;
  }
  throw new Error(`spawned tab ${tabId} has no workspace tab`);
}

async function emitStaleBootStage(
  page: Page,
  tabId: string,
  stage: "spawn_failed" | "pty_reattached",
): Promise<void> {
  await page.evaluate(
    async ({ tabId, stage }) => {
      const eventApi = (window as unknown as {
        __TAURI__?: { event?: { emit?: (name: string, payload: unknown) => Promise<void> } };
      }).__TAURI__?.event;
      if (!eventApi?.emit) throw new Error("Tauri event bridge is unavailable");
      await eventApi.emit("tab-boot-stage", {
        tab_id: tabId,
        stage,
        reason: stage === "spawn_failed" ? "stale Playwright launch result" : null,
      });
    },
    { tabId, stage },
  );
}

test("@live attached Codex Chat ignores a late spawn-failed event without opening Terminal", async ({
  api,
  ui,
  spawnAgent,
}) => {
  const title = `pw-live-stale-boot-${Date.now()}`;
  const spawned = await spawnAgent<SpawnedTab>({
    agent: "codex",
    model: "gpt-5.4-mini",
    title,
    skip_permissions: true,
    activate: true,
  });
  expect(spawned.agent).toBe("codex");

  await activateTabChat(api, spawned.tab_id);

  const chat = ui.page.getByRole("region", { name: `Tab chat: ${title}` });
  const composer = chat.getByRole("textbox", { name: "Message composer" });
  await expect(chat).toBeVisible({ timeout: 30_000 });
  await expect(composer).toBeVisible({ timeout: 30_000 });
  await expect(chat).toHaveAttribute("aria-busy", "false", { timeout: 30_000 });

  const tab = (await api.get<Array<{ id: string; pty_id?: string | null; agent_type?: string | null }>>("/tabs"))
    .find((candidate) => candidate.id === spawned.tab_id);
  expect(tab?.agent_type).toBe("codex");
  expect(tab?.pty_id).toBeTruthy();

  await emitStaleBootStage(ui.page, spawned.tab_id, "spawn_failed");

  await expect(ui.page.getByRole("alert", { name: "Launch failed" })).toHaveCount(0);
  await expect(composer).toBeEnabled();
  await composer.fill("stale boot event must not interrupt this draft");
  await expect(composer).toHaveValue("stale boot event must not interrupt this draft");

  await emitStaleBootStage(ui.page, spawned.tab_id, "pty_reattached");
  await expect(ui.page.getByRole("alert", { name: "Launch failed" })).toHaveCount(0);
  await expect(composer).toHaveValue("stale boot event must not interrupt this draft");
});
