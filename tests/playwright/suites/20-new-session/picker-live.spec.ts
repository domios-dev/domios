/**
 * WHAT: real-provider template routes launch their declared agent surfaces:
 *       orchestration pair and a single agent.
 * WHY: golden uses the replay transport, so it cannot catch a provider launch
 *      that falls back to a bare shell or starts work without instructions.
 * ORACLE: every returned tab gains a live PTY/session and its first chat
 *         message; a taskless pair stays idle until an explicit post-ready probe.
 */
import { expect, test } from "../../helpers/fixtures";
import type { ApiClient } from "../../helpers/api-client";
import {
  assertRealTemplateSpawn,
  expectRealSpawnIdle,
  expectFreshAssistantReply,
} from "../../helpers/real-spawn-smoke";

type PairResult = {
  plan_id?: string;
  orchestrator_tab_id: string;
  planner_tab_id: string;
};

async function createTestWorkspace(
  api: ApiClient,
): Promise<{ id: string; name: string }> {
  const name = `pw-live-pair-${Date.now()}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find(
    (candidate) => candidate.name === name,
  );
  if (!ws) throw new Error(`workspace ${name} missing after create`);
  await api.focusWorkspace(ws.id);
  return { id: ws.id, name };
}

async function tabText(api: ApiClient, tabId: string): Promise<string> {
  const text = await api
    .get<{ text?: string } | string>(`/tabs/${tabId}/text`)
    .catch(() => "");
  return typeof text === "string" ? text : (text.text ?? JSON.stringify(text));
}

test("@live real orchestration-pair template spawn materializes both providers and waits idle", async ({
  api,
  ui,
  tabCleanup,
}) => {
  const ws = await createTestWorkspace(api);
  const pair = await api.post<PairResult>(
    "/orchestration/new",
    {
      orchestrator_agent: "claude",
      orchestrator_model: "sonnet",
      planner_agent: "claude",
      planner_model: "sonnet",
      workspace_id: ws.id,
      title: `pw-live-pair-${Date.now()}`,
    },
  );
  tabCleanup.track(pair.orchestrator_tab_id);
  tabCleanup.track(pair.planner_tab_id);

  expect(
    pair.plan_id,
    "pair launch stays lazy until the Orchestrator receives a goal",
  ).toBeUndefined();

  await assertRealTemplateSpawn(api, ui.page, "orchestration-pair", [
    {
      tabId: pair.orchestrator_tab_id,
      provider: "claude",
      label: "Orchestrator",
    },
    { tabId: pair.planner_tab_id, provider: "claude", label: "Planner" },
  ]);

  const plannerStatus = await api.get<Record<string, unknown>>(
    `/tabs/${pair.planner_tab_id}/status`,
  );
  expect(plannerStatus.parent_tab_id).toBe(pair.orchestrator_tab_id);
  expect(plannerStatus.role).toBe("Manager");

  await Promise.all([
    expectRealSpawnIdle(api, ui.page, {
      tabId: pair.orchestrator_tab_id,
      provider: "claude",
      label: "Orchestrator",
    }),
    expectRealSpawnIdle(api, ui.page, {
      tabId: pair.planner_tab_id,
      provider: "claude",
      label: "Planner",
    }),
  ]);

  const orchestratorText = await tabText(api, pair.orchestrator_tab_id);
  expect(orchestratorText).not.toContain(
    `Your Planner specialist is live: tab ${pair.planner_tab_id}`,
  );

  await expectFreshAssistantReply(
    api,
    ui.page,
    {
      tabId: pair.orchestrator_tab_id,
      provider: "claude",
      label: "Orchestrator",
    },
    "Return only the decimal result of seventeen plus six.",
    "23",
  );
});

test("@live real single-agent template spawn materializes a provider and its first chat message", async ({
  api,
  ui,
  tabCleanup,
  spawnAgent,
}) => {
  const spawned = await spawnAgent<{ tab_id: string; agent: string }>({
    agent: "claude",
    model: "sonnet",
    title: "pw-live-single-template",
    skip_permissions: true,
    activate: true,
  });
  expect(spawned.agent).toBe("claude");

  await assertRealTemplateSpawn(api, ui.page, "single-agent", [
    { tabId: spawned.tab_id, provider: "claude", label: "Single agent" },
  ]);

  await expectFreshAssistantReply(
    api,
    ui.page,
    { tabId: spawned.tab_id, provider: "claude", label: "Single agent" },
    "Return only the decimal result of nineteen plus eight.",
    "27",
  );
});
