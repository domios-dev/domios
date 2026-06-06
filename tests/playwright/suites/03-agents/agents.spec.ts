/**
 * 03-agents — agent lifecycle against the live dev console.
 *
 * Strategy (Andrey's Q2 ruling): NO mocks, NO HTTP intercept — real LLM,
 * real-system fidelity. Consequences baked in here:
 * - @smoke tier is LLM-free: spawn validation + the report-IPC primitive
 *   exercised with plain shell panes wired via /role (parent/child).
 * - @golden tier makes exactly ONE real LLM call (sonnet, trivial task)
 *   and asserts STRUCTURALLY: spawn shape, pane agent_type, report
 *   arrival containing a unique marker — never exact response text.
 * - Generous-but-bounded timeouts for real model latency.
 *
 * Auth: the spawned agent uses the host's provider CLI login (claude CLI
 * on this machine) — no API key env needed for the claude provider.
 */
import { expect, test } from "@playwright/test";
import { ApiClient } from "../../helpers/api-client";
import { apiReset } from "../../helpers/reset";

const api = new ApiClient();

test.afterAll(async () => {
  await apiReset(api);
});

// ── @smoke: no LLM involved ──────────────────────────────────────────

test("@smoke spawn with an unknown provider errors honestly", async () => {
  let status = 0;
  let message = "";
  try {
    await api.post("/agents/spawn", { agent: "definitely-no-such-provider" });
  } catch (e) {
    message = String(e);
    status = Number(/HTTP (\d+)/.exec(message)?.[1] ?? 0);
  }
  expect(status, "spawn must not 2xx for an unknown provider").toBeGreaterThanOrEqual(400);
  expect(message).toContain("definitely-no-such-provider");
});

test("@smoke report IPC primitive: child report lands in parent inbox (no LLM)", async () => {
  const parent = (await api.createPane({ title: "pw-report-parent" })) as { id: string };
  const child = (await api.createPane({ title: "pw-report-child" })) as { id: string };
  try {
    // 2-level hierarchy: a Worker's parent must be an Orchestrator.
    await api.post(`/panes/${parent.id}/role`, { role: "Orchestrator" });
    await api.post(`/panes/${child.id}/role`, {
      role: "Worker",
      parent_pane_id: parent.id,
    });

    const marker = `pw-report-${Date.now()}`;
    const submitted = await api.post<{ ok: boolean; report_id: string }>(
      `/panes/${child.id}/report`,
      { message: marker },
    );
    expect(submitted.ok).toBe(true);
    expect(submitted.report_id).toBeTruthy();

    const reports = await api.get<unknown>(`/panes/${parent.id}/reports?clear=true`);
    const text = JSON.stringify(reports);
    expect(text, `parent inbox missing the report: ${text.slice(0, 300)}`).toContain(marker);
    expect(text, "report must carry the reporting pane id").toContain(child.id);
  } finally {
    await api.closePane(child.id).catch(() => {});
    await api.closePane(parent.id).catch(() => {});
  }
});

test("@smoke pane status endpoint answers for a live pane and 404s for a dead id", async () => {
  const pane = (await api.createPane({ title: "pw-status-probe" })) as { id: string };
  try {
    const status = await api.get<Record<string, unknown>>(`/panes/${pane.id}/status`);
    expect(typeof status).toBe("object");
  } finally {
    await api.closePane(pane.id).catch(() => {});
  }
  let code = 200;
  try {
    await api.get("/panes/no-such-pane/status");
  } catch (e) {
    code = Number(/HTTP (\d+)/.exec(String(e))?.[1] ?? 0);
  }
  expect(code).toBe(404);
});

// ── @golden: ONE real LLM call ───────────────────────────────────────

test("@golden real agent spawn → works → reports back (structural asserts)", async () => {
  // Real model latency + CLI startup; bounded, not unbounded.
  test.setTimeout(300_000);

  const inbox = (await api.createPane({ title: "pw-agent-inbox" })) as { id: string };
  const marker = `PW-AGENT-OK-${Date.now()}`;
  let agentPaneId: string | undefined;

  try {
    // 2-level hierarchy: a Worker must hang off an Orchestrator parent.
    await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await api.post<{
      pane_id: string;
      agent: string;
      role?: string;
    }>("/agents/spawn", {
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-agent-golden",
      parent_pane_id: inbox.id,
      report_target_pane_id: inbox.id,
      skip_permissions: true,
      task:
        `Run this exact command and then stop: codeterm agent report "${marker}". ` +
        "Do not do anything else. Do not explain.",
    });
    agentPaneId = spawned.pane_id;
    expect(agentPaneId).toBeTruthy();
    expect(spawned.agent).toBe("claude");

    // Structural: the pane exists and is typed as a claude agent pane.
    const pane = (await api.panes()).find((p) => p.id === agentPaneId);
    expect(pane, "spawned agent pane missing from pane list").toBeDefined();
    expect(pane!.agent_type).toBe("claude");

    // The agent runs the report command; poll the inbox. Single LLM call,
    // no retries — if the model never reports within budget, that is a
    // real finding, not flake to paper over.
    let reportText = "";
    const deadline = Date.now() + 240_000;
    while (Date.now() < deadline) {
      const reports = await api.get<unknown>(`/panes/${inbox.id}/reports`);
      reportText = JSON.stringify(reports);
      if (reportText.includes(marker)) break;
      await new Promise((r) => setTimeout(r, 3_000));
    }
    expect(reportText, "agent report with marker never arrived").toContain(marker);

    // Session detection is async but should have landed by report time.
    const status = await api.get<Record<string, unknown>>(`/panes/${agentPaneId}/status`);
    expect(JSON.stringify(status).length).toBeGreaterThan(2);
  } finally {
    if (agentPaneId) await api.closePane(agentPaneId).catch(() => {});
    await api.closePane(inbox.id).catch(() => {});
  }
});
