/**
 * WHAT: real-provider template routes launch their declared agent surfaces:
 *       orchestration pair and a single agent.
 * WHY: golden uses the replay transport, so it cannot catch a provider launch
 *      that falls back to a bare shell or starts work without instructions.
 * ORACLE: every returned pane gains a live PTY/session and its first chat
 *         message; a taskless pair stays idle until an explicit post-ready probe.
 */
import { expect, test } from "../../helpers/fixtures";
import type { ApiClient } from "../../helpers/api-client";
import {
  assertRealTemplateSpawn,
  expectFreshAssistantReply,
} from "../../helpers/real-spawn-smoke";

type PairResult = {
  plan_id?: string;
  orchestrator_pane_id: string;
  planner_pane_id: string;
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

async function paneText(api: ApiClient, paneId: string): Promise<string> {
  const text = await api
    .get<{ text?: string } | string>(`/panes/${paneId}/text`)
    .catch(() => "");
  return typeof text === "string" ? text : (text.text ?? JSON.stringify(text));
}

test("@live real orchestration-pair template spawn materializes both providers and waits idle", async ({
  api,
  ui,
  paneCleanup,
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
  paneCleanup.track(pair.orchestrator_pane_id);
  paneCleanup.track(pair.planner_pane_id);

  expect(
    pair.plan_id,
    "pair launch stays lazy until the Orchestrator receives a goal",
  ).toBeUndefined();

  await assertRealTemplateSpawn(api, ui.page, "orchestration-pair", [
    {
      paneId: pair.orchestrator_pane_id,
      provider: "claude",
      label: "Orchestrator",
    },
    { paneId: pair.planner_pane_id, provider: "claude", label: "Planner" },
  ]);

  const plannerStatus = await api.get<Record<string, unknown>>(
    `/panes/${pair.planner_pane_id}/status`,
  );
  expect(plannerStatus.parent_pane_id).toBe(pair.orchestrator_pane_id);
  expect(plannerStatus.role).toBe("Manager");

  for (const paneId of [pair.orchestrator_pane_id, pair.planner_pane_id]) {
    const status = await api.get<Record<string, unknown>>(
      `/panes/${paneId}/status`,
    );
    expect(status.status, `taskless manager ${paneId} stays idle`).toBe("Idle");
  }

  const orchestratorText = await paneText(api, pair.orchestrator_pane_id);
  expect(orchestratorText).not.toContain(
    `Your Planner specialist is live: pane ${pair.planner_pane_id}`,
  );

  await expectFreshAssistantReply(
    api,
    ui.page,
    {
      paneId: pair.orchestrator_pane_id,
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
  paneCleanup,
  spawnAgent,
}) => {
  const spawned = await spawnAgent<{ pane_id: string; agent: string }>({
    agent: "claude",
    model: "sonnet",
    title: "pw-live-single-template",
    skip_permissions: true,
    activate: true,
  });
  expect(spawned.agent).toBe("claude");

  await assertRealTemplateSpawn(api, ui.page, "single-agent", [
    { paneId: spawned.pane_id, provider: "claude", label: "Single agent" },
  ]);

  await expectFreshAssistantReply(
    api,
    ui.page,
    { paneId: spawned.pane_id, provider: "claude", label: "Single agent" },
    "Return only the decimal result of nineteen plus eight.",
    "27",
  );
});
