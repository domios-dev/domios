/**
 * WHAT: the real-provider orchestration pair route launches both Claude PTYs
 *       and delivers the post-launch Planner follow-up to the Orchestrator.
 * WHY: golden uses the replay transport, so it cannot catch a provider launch
 *      that falls back to a bare shell or loses the follow-up during startup.
 * ORACLE: both pane-local session ids materialize and the Orchestrator buffer
 *      contains the exact live Planner pane id from the real pair response.
 */
import { expect, test } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";
import type { ApiClient } from "../../helpers/api-client";

type PairResult = {
  plan_id?: string;
  orchestrator_pane_id: string;
  planner_pane_id: string;
};

async function createTestWorkspace(api: ApiClient): Promise<{ id: string; name: string }> {
  const name = `pw-live-pair-${Date.now()}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find((candidate) => candidate.name === name);
  if (!ws) throw new Error(`workspace ${name} missing after create`);
  await api.focusWorkspace(ws.id);
  return { id: ws.id, name };
}

async function paneText(api: ApiClient, paneId: string): Promise<string> {
  const text = await api.get<{ text?: string } | string>(`/panes/${paneId}/text`).catch(() => "");
  return typeof text === "string" ? text : (text.text ?? JSON.stringify(text));
}

async function sessionId(api: ApiClient, paneId: string): Promise<string | undefined> {
  const status: Record<string, unknown> = await api
    .get<Record<string, unknown>>(`/panes/${paneId}/status`)
    .catch(() => ({} as Record<string, unknown>));
  const value = status.agent_session_id;
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

test("@golden @live real pair launch leaves no bare shell and delivers the Planner follow-up", async ({
  api,
  paneCleanup,
}) => {
  test.setTimeout(300_000);
  const ws = await createTestWorkspace(api);
  const pair = await api.post<PairResult>("/orchestration/new", {
    orchestrator_agent: "claude",
    orchestrator_model: "sonnet",
    planner_agent: "claude",
    planner_model: "sonnet",
    workspace_id: ws.id,
    title: `pw-live-pair-${Date.now()}`,
  });
  paneCleanup.track(pair.orchestrator_pane_id);
  paneCleanup.track(pair.planner_pane_id);

  expect(pair.plan_id, "pair launch stays lazy until the Orchestrator receives a goal").toBeUndefined();

  const [orchestratorSession, plannerSession] = await Promise.all([
    pollUntil(
      () => sessionId(api, pair.orchestrator_pane_id),
      180_000,
      1_000,
      "real Orchestrator provider session materializes",
    ),
    pollUntil(
      () => sessionId(api, pair.planner_pane_id),
      180_000,
      1_000,
      "real Planner provider session materializes",
    ),
  ]);
  expect(orchestratorSession).toBeTruthy();
  expect(plannerSession).toBeTruthy();

  const followup = `Your Planner specialist is live: pane ${pair.planner_pane_id}`;
  const orchestratorText = await pollUntil(
    async () => {
      const text = await paneText(api, pair.orchestrator_pane_id);
      return text.includes(followup) ? text : undefined;
    },
    60_000,
    500,
    "pair follow-up reaches the launched Orchestrator PTY",
  );
  expect(orchestratorText).toContain(followup);

  const plannerStatus = await api.get<Record<string, unknown>>(
    `/panes/${pair.planner_pane_id}/status`,
  );
  expect(plannerStatus.parent_pane_id).toBe(pair.orchestrator_pane_id);
  expect(plannerStatus.role).toBe("Manager");

  const combinedOutput = `${orchestratorText}\n${await paneText(api, pair.planner_pane_id)}`;
  expect(combinedOutput, "@live must never run through replay.mjs").not.toContain("stub replay");
});
