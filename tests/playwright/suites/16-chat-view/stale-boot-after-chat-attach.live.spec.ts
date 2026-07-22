/**
 * WHAT: Attached Codex Chat remains authoritative after stale boot-stage events.
 * WHY: Late spawn failures must not cover a live conversation or interrupt its composer.
 * ORACLE: Chat stays visible through spawn_failed and pty_reattached events.
 */
import type { Page } from "@playwright/test";
import { expect, test } from "../../helpers/fixtures";

type SpawnedPane = { pane_id: string; agent: string };
type MuxState = {
  workspaces?: Array<{ id: string; tabs?: Array<{ id: string; pane_id: string }> }>;
};

async function activatePaneChat(
  api: { get: <T>(path: string) => Promise<T>; post: (path: string, body?: unknown) => Promise<unknown> },
  paneId: string,
): Promise<void> {
  const mux = await api.get<MuxState>("/mux/state");
  for (const workspace of mux.workspaces ?? []) {
    const tab = workspace.tabs?.find((candidate) => candidate.pane_id === paneId);
    if (!tab) continue;
    await api.post(`/panes/${paneId}/chat-view`, { enabled: true });
    await api.post(`/workspaces/${workspace.id}/focus`);
    await api.post(`/workspaces/${workspace.id}/tabs/${tab.id}/focus`);
    await api.post(`/panes/${paneId}/focus`);
    return;
  }
  throw new Error(`spawned pane ${paneId} has no workspace tab`);
}

async function emitStaleBootStage(
  page: Page,
  paneId: string,
  stage: "spawn_failed" | "pty_reattached",
): Promise<void> {
  await page.evaluate(
    async ({ paneId, stage }) => {
      const eventApi = (window as unknown as {
        __TAURI__?: { event?: { emit?: (name: string, payload: unknown) => Promise<void> } };
      }).__TAURI__?.event;
      if (!eventApi?.emit) throw new Error("Tauri event bridge is unavailable");
      await eventApi.emit("pane-boot-stage", {
        pane_id: paneId,
        stage,
        reason: stage === "spawn_failed" ? "stale Playwright launch result" : null,
      });
    },
    { paneId, stage },
  );
}

test("@live attached Codex Chat ignores a late spawn-failed event without opening Terminal", async ({
  api,
  ui,
  spawnAgent,
}) => {
  const title = `pw-live-stale-boot-${Date.now()}`;
  const spawned = await spawnAgent<SpawnedPane>({
    agent: "codex",
    model: "gpt-5.4-mini",
    title,
    skip_permissions: true,
    activate: true,
  });
  expect(spawned.agent).toBe("codex");

  await activatePaneChat(api, spawned.pane_id);

  const chat = ui.page.getByRole("region", { name: `Pane chat: ${title}` });
  const composer = chat.getByRole("textbox", { name: "Message composer" });
  await expect(chat).toBeVisible({ timeout: 30_000 });
  await expect(composer).toBeVisible({ timeout: 30_000 });
  await expect(chat).toHaveAttribute("aria-busy", "false", { timeout: 30_000 });

  const pane = (await api.get<Array<{ id: string; pty_id?: string | null; agent_type?: string | null }>>("/panes"))
    .find((candidate) => candidate.id === spawned.pane_id);
  expect(pane?.agent_type).toBe("codex");
  expect(pane?.pty_id).toBeTruthy();

  await emitStaleBootStage(ui.page, spawned.pane_id, "spawn_failed");

  await expect(ui.page.getByRole("alert", { name: "Launch failed" })).toHaveCount(0);
  await expect(composer).toBeEnabled();
  await composer.fill("stale boot event must not interrupt this draft");
  await expect(composer).toHaveValue("stale boot event must not interrupt this draft");

  await emitStaleBootStage(ui.page, spawned.pane_id, "pty_reattached");
  await expect(ui.page.getByRole("alert", { name: "Launch failed" })).toHaveCount(0);
  await expect(composer).toHaveValue("stale boot event must not interrupt this draft");
});
