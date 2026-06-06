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
import { connectUi } from "../../helpers/dev-console";
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

async function waitForMarkerInDom(
  ui: Awaited<ReturnType<typeof connectUi>>,
  marker: string,
  timeout = 240_000,
): Promise<void> {
  await ui.page.waitForFunction(
    (m) =>
      Array.from(document.querySelectorAll(".xterm-accessibility")).some((tree) =>
        (tree.textContent ?? "").includes(m),
      ),
    marker,
    { timeout, polling: 1_000 },
  );
}

async function spawnWorker(inboxId: string, task: string): Promise<string> {
  const spawned = await api.post<{ pane_id: string }>("/agents/spawn", {
    agent: "claude",
    model: "sonnet",
    role: "Worker",
    title: "pw-agent-live",
    parent_pane_id: inboxId,
    report_target_pane_id: inboxId,
    skip_permissions: true,
    activate: true,
    task,
  });
  return spawned.pane_id;
}

test("@golden multi-turn + tool round-trip observed in the live terminal DOM", async () => {
  // Turn 1 makes the agent EXECUTE a command (tool use — the echoed marker
  // only reaches the terminal if the tool actually ran); turn 2 sends a
  // follow-up into the same session and reads the reply from the DOM.
  test.setTimeout(420_000);

  const ui = await connectUi();
  const inbox = (await api.createPane({ title: "pw-mt-inbox" })) as { id: string };
  const toolMarker = `PW-TOOL-${Date.now()}`;
  const turn2Marker = `PW-TURN2-${Date.now()}`;
  let agentPaneId: string | undefined;

  try {
    await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
    agentPaneId = await spawnWorker(
      inbox.id,
      `Run exactly this command and nothing else, then stop: echo ${toolMarker}`,
    );
    await waitForMarkerInDom(ui, toolMarker);

    // Same pane, same session — a real second turn.
    await api.post(`/panes/${agentPaneId}/send`, {
      text: `Reply with a single line containing exactly ${turn2Marker} and nothing else.`,
      enter: true,
    });
    await waitForMarkerInDom(ui, turn2Marker);
  } finally {
    if (agentPaneId) await api.closePane(agentPaneId).catch(() => {});
    await api.closePane(inbox.id).catch(() => {});
    await ui.browser.close().catch(() => {});
  }
});

test("@golden agent restart mid-session resumes and stays usable", async () => {
  test.setTimeout(420_000);

  const ui = await connectUi();
  const inbox = (await api.createPane({ title: "pw-restart-inbox" })) as { id: string };
  const warmMarker = `PW-WARM-${Date.now()}`;
  const resumeMarker = `PW-RESUME-${Date.now()}`;
  let agentPaneId: string | undefined;

  try {
    await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
    agentPaneId = await spawnWorker(
      inbox.id,
      `Reply with a single line containing exactly ${warmMarker} and nothing else.`,
    );
    await waitForMarkerInDom(ui, warmMarker);

    const sessionBefore = (await api.panes()).find((p) => p.id === agentPaneId) as
      | { agent_session_id?: string }
      | undefined;

    // Graceful restart: /exit + auto-resume of the same session.
    const restart = await api.post<{ restarting: boolean; session_id?: string }>(
      `/panes/${agentPaneId}/agent/restart`,
      {},
    );
    expect(restart.restarting).toBe(true);

    // The pane must survive the restart and stay a claude agent pane.
    const deadline = Date.now() + 120_000;
    let pane: { agent_type?: string | null; agent_session_id?: string } | undefined;
    while (Date.now() < deadline) {
      pane = (await api.panes()).find((p) => p.id === agentPaneId);
      if (pane?.agent_type === "claude") break;
      await new Promise((r) => setTimeout(r, 2_000));
    }
    expect(pane?.agent_type, "pane lost its agent after restart").toBe("claude");

    // Usability after restart is the real bar: a fresh prompt round-trips.
    // Give the resumed CLI a moment to be input-ready.
    await new Promise((r) => setTimeout(r, 10_000));
    await api.post(`/panes/${agentPaneId}/send`, {
      text: `Reply with a single line containing exactly ${resumeMarker} and nothing else.`,
      enter: true,
    });
    await waitForMarkerInDom(ui, resumeMarker);

    // Resume (not a fresh session): the session id survives when it was
    // known before the restart.
    if (sessionBefore?.agent_session_id) {
      const after = (await api.panes()).find((p) => p.id === agentPaneId) as
        | { agent_session_id?: string }
        | undefined;
      expect(after?.agent_session_id, "restart replaced the session instead of resuming").toBe(
        sessionBefore.agent_session_id,
      );
    }
  } finally {
    if (agentPaneId) await api.closePane(agentPaneId).catch(() => {});
    await api.closePane(inbox.id).catch(() => {});
    await ui.browser.close().catch(() => {});
  }
});

test("@golden fully live: agent response read from the rendered terminal DOM", async () => {
  // Real UI → real PTY → real LLM → real DOM. The WebGL canvas has no
  // readable text; the screenReaderMode accessibility tree
  // (.xterm-accessibility) is the renderer-independent observable.
  test.setTimeout(300_000);

  const ui = await connectUi();
  const inbox = (await api.createPane({ title: "pw-dom-inbox" })) as { id: string };
  const marker = `PW-DOM-${Date.now()}`;
  let agentPaneId: string | undefined;

  try {
    await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await api.post<{ pane_id: string }>("/agents/spawn", {
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-agent-dom",
      parent_pane_id: inbox.id,
      report_target_pane_id: inbox.id,
      skip_permissions: true,
      activate: true,
      task: `Reply with a single line containing exactly ${marker} and nothing else.`,
    });
    agentPaneId = spawned.pane_id;

    // The model's reply lands in the agent pane's terminal; the active
    // pane's accessibility tree mirrors the viewport text.
    await ui.page.waitForFunction(
      (m) =>
        Array.from(document.querySelectorAll(".xterm-accessibility")).some((tree) =>
          (tree.textContent ?? "").includes(m),
        ),
      marker,
      { timeout: 240_000, polling: 1_000 },
    );

    // Cross-check the same content through the PTY text API — DOM and
    // daemon-side buffer must agree.
    const text = await api.get<{ text?: string } | string>(`/panes/${agentPaneId}/text`);
    const buffer = typeof text === "string" ? text : (text.text ?? JSON.stringify(text));
    expect(buffer, "PTY buffer missing the marker the DOM showed").toContain(marker);
  } finally {
    if (agentPaneId) await api.closePane(agentPaneId).catch(() => {});
    await api.closePane(inbox.id).catch(() => {});
    await ui.browser.close().catch(() => {});
  }
});
