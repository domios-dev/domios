/**
 * WHAT: agents covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 03-agents.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type SpawnAgent } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";

// ── @smoke: no LLM involved ──────────────────────────────────────────

test("@smoke spawn with an unknown provider errors honestly", async ({
  api,
}) => {
  let status = 0;
  let message = "";
  try {
    await api.post("/agents/spawn", { agent: "definitely-no-such-provider" });
  } catch (e) {
    message = String(e);
    status = Number(/HTTP (\d+)/.exec(message)?.[1] ?? 0);
  }
  expect(
    status,
    "spawn must not 2xx for an unknown provider",
  ).toBeGreaterThanOrEqual(400);
  expect(message).toContain("definitely-no-such-provider");
});

test("@smoke report IPC primitive: child report lands in parent inbox (no LLM)", async ({
  api,
  paneCleanup,
}) => {
  const parent = await paneCleanup.createPane({ title: "pw-report-parent" });
  const child = await paneCleanup.createPane({ title: "pw-report-child" });
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

  const reports = await api.get<unknown>(
    `/panes/${parent.id}/reports?clear=true`,
  );
  const text = JSON.stringify(reports);
  expect(
    text,
    `parent inbox missing the report: ${text.slice(0, 300)}`,
  ).toContain(marker);
  expect(text, "report must carry the reporting pane id").toContain(child.id);
});

test("@smoke pane status endpoint answers for a live pane and 404s for a dead id", async ({
  api,
  paneCleanup,
}) => {
  const pane = await paneCleanup.createPane({ title: "pw-status-probe" });
  const status = await api.get<Record<string, unknown>>(
    `/panes/${pane.id}/status`,
  );
  expect(typeof status).toBe("object");
  await api.closePane(pane.id).catch(() => {});
  let code = 200;
  try {
    await api.get("/panes/no-such-pane/status");
  } catch (e) {
    code = Number(/HTTP (\d+)/.exec(String(e))?.[1] ?? 0);
  }
  expect(code).toBe(404);
});

test("@smoke make-orchestrator rejects plain shell panes", async ({
  api,
  paneCleanup,
}) => {
  const pane = await paneCleanup.createPane({ title: "pw-make-orch-shell" });
  let status = 0;
  try {
    await api.post(`/panes/${pane.id}/make-orchestrator`, {});
  } catch (e) {
    status = Number(/HTTP (\d+)/.exec(String(e))?.[1] ?? 0);
  }
  expect(status).toBe(400);
});

test("@smoke make-orchestrator promotes agent pane and surfaces onboarding", async ({
  api,
  paneCleanup,
}) => {
  const pane = await paneCleanup.createPane({
    title: "pw-make-orch-agent",
    eager: true,
  });
  await api.post(`/panes/${pane.id}/agent-type`, { agent_type: "claude" });

  const promoted = await api.post<{ ok: boolean }>(
    `/panes/${pane.id}/make-orchestrator`,
    {},
  );
  expect(promoted.ok).toBe(true);

  const state = await api.get<{
    panes: Record<string, { role?: string; can_spawn?: boolean }>;
  }>("/mux/state");
  const live = state.panes[pane.id];
  expect(live?.role).toBe("Orchestrator");
  expect(live?.can_spawn).toBe(true);

  const chat = await api.post<{ messages?: { content?: string }[] }>(
    `/panes/${pane.id}/chat-raw/start`,
    {},
  );
  const greeting = JSON.stringify(chat.messages ?? []);
  expect(greeting).toContain("You are now the Orchestrator");
  expect(greeting).toContain("codeterm doc orchestration");
  expect(greeting).toContain(`codeterm agent spawn --parent ${pane.id}`);
});

// ── @golden: stub transport (real spawn/report/DOM pipeline, no LLM) ──
// Original live-LLM variants: agents-live.spec.ts (@live project).

import { tempSpawnCwd } from "../../helpers/spawn-cwd";
import {
  isStubTransportActive,
  showTerminalView,
} from "../../helpers/stub-gate";

const MATRIX = ["claude", "codex", "opencode"] as const;

test.describe("agent flows on the stub transport", () => {
  test.beforeEach(async ({ api }) => {
    test.skip(
      !(await isStubTransportActive(api)),
      "dev daemon is not running with CODETERM_STUB_AGENT_CMD",
    );
  });

  for (const provider of MATRIX) {
    test(`@golden ${provider}: agent spawn → works → reports back (structural asserts)`, async ({
      api,
      paneCleanup,
      spawnAgent,
    }) => {
      test.setTimeout(90_000);
      const inbox = await paneCleanup.createPane({ title: "pw-agent-inbox" });
      const marker = `PW-AGENT-OK-${Date.now()}`;
      let agentPaneId: string | undefined;
      try {
        await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
        const spawned = await spawnAgent<{ pane_id: string; agent: string }>({
          agent: provider,
          role: "Worker",
          title: "pw-agent-golden",
          parent_pane_id: inbox.id,
          report_target_pane_id: inbox.id,
          skip_permissions: true,
          cwd: tempSpawnCwd("pw-stub-agent-"),
          task: `STUB_REPORT ${marker}`,
        });
        agentPaneId = spawned.pane_id;
        expect(agentPaneId).toBeTruthy();
        expect(spawned.agent).toBe(provider);

        const pane = (await api.panes()).find((p) => p.id === agentPaneId);
        expect(pane, "spawned agent pane missing from pane list").toBeDefined();
        expect(pane!.agent_type).toBe(provider);

        const reportText = await pollUntil(
          async () => {
            const reports = await api.get<unknown>(
              `/panes/${inbox.id}/reports`,
            );
            const text = JSON.stringify(reports);
            return text.includes(marker) ? text : undefined;
          },
          60_000,
          500,
          "agent report reaches inbox",
        );
        expect(reportText, "agent report with marker never arrived").toContain(
          marker,
        );

        const status = await api.get<Record<string, unknown>>(
          `/panes/${agentPaneId}/status`,
        );
        expect(status.role).toBe("Worker");
        expect(status.parent_pane_id).toBe(inbox.id);
        expect(status.agent_type).toBe(provider);
        const sid = await pollUntil(
          async () => {
            const panes = await api.panes();
            return (
              panes.find((p) => p.id === agentPaneId) as
                { agent_session_id?: string } | undefined
            )?.agent_session_id;
          },
          30_000,
          500,
          "session id materializes",
        );
        expect(sid ?? "").not.toBe("");
      } finally {
        if (agentPaneId) paneCleanup.track(agentPaneId);
      }
    });

    test(`@golden ${provider}: multi-turn + tool round-trip observed in the live terminal DOM`, async ({
      api,
      paneCleanup,
      spawnAgent,
      pollMarkerInDom,
      ui,
    }) => {
      test.setTimeout(90_000);
      const inbox = await paneCleanup.createPane({ title: "pw-mt-inbox" });
      const toolMarker = `PWTOOL${Date.now()}`;
      const turn2Marker = `PWTURN2${Date.now()}`;
      let agentPaneId: string | undefined;
      try {
        await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
        const spawned = await spawnAgent({
          agent: provider,
          role: "Worker",
          title: "pw-agent-live",
          parent_pane_id: inbox.id,
          report_target_pane_id: inbox.id,
          skip_permissions: true,
          activate: true,
          cwd: tempSpawnCwd("pw-stub-agent-"),
          task: `STUB_TOOL ${toolMarker}`,
        });
        agentPaneId = spawned.pane_id;
        await showTerminalView(ui.page, agentPaneId);
        const toolReply = `tool round-trip done ${toolMarker}`;
        await pollMarkerInDom(toolReply, {
          paneId: agentPaneId,
          timeout: 60_000,
        });
        const toolText = await api.get<{ text?: string } | string>(
          `/panes/${agentPaneId}/text`,
        );
        const toolBuffer =
          typeof toolText === "string"
            ? toolText
            : (toolText.text ?? JSON.stringify(toolText));
        expect(
          toolBuffer,
          "stub tool reply missing from pane buffer",
        ).toContain(toolReply);

        await api.post(`/panes/${agentPaneId}/send`, {
          text: `STUB_ECHO ${turn2Marker}`,
          enter: true,
          raw: true,
        });
        const turn2Reply = `ack ${turn2Marker}`;
        await pollMarkerInDom(turn2Reply, {
          paneId: agentPaneId,
          timeout: 60_000,
        });
        const turn2Buffer = await pollUntil(
          async () => {
            const turn2Text = await api.get<{ text?: string } | string>(
              `/panes/${agentPaneId}/text`,
            );
            const buffer =
              typeof turn2Text === "string"
                ? turn2Text
                : (turn2Text.text ?? JSON.stringify(turn2Text));
            return buffer.includes(turn2Reply) ? buffer : undefined;
          },
          10_000,
          250,
          "second stub reply reaches pane text buffer",
        );
        expect(
          turn2Buffer,
          "second stub reply missing from pane buffer",
        ).toContain(turn2Reply);
      } finally {
        if (agentPaneId) paneCleanup.track(agentPaneId);
      }
    });

    test(`@golden ${provider}: agent response read from the rendered terminal DOM`, async ({
      api,
      paneCleanup,
      spawnAgent,
      pollMarkerInDom,
      ui,
    }) => {
      test.setTimeout(90_000);
      const inbox = await paneCleanup.createPane({ title: "pw-dom-inbox" });
      const marker = `PWDOM${Date.now()}`;
      let agentPaneId: string | undefined;
      try {
        await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
        const spawned = await spawnAgent({
          agent: provider,
          role: "Worker",
          title: "pw-agent-dom",
          parent_pane_id: inbox.id,
          report_target_pane_id: inbox.id,
          skip_permissions: true,
          activate: true,
          cwd: tempSpawnCwd("pw-stub-agent-"),
          task: `STUB_ECHO ${marker}`,
        });
        agentPaneId = spawned.pane_id;
        await showTerminalView(ui.page, agentPaneId);
        const reply = `ack ${marker}`;
        await pollMarkerInDom(reply, { paneId: agentPaneId, timeout: 60_000 });
        const text = await api.get<{ text?: string } | string>(
          `/panes/${agentPaneId}/text`,
        );
        const buffer =
          typeof text === "string" ? text : (text.text ?? JSON.stringify(text));
        expect(
          buffer,
          "PTY buffer missing the stub reply the DOM showed",
        ).toContain(reply);
      } finally {
        if (agentPaneId) paneCleanup.track(agentPaneId);
      }
    });

    test(`@golden ${provider}: agent restart mid-session resumes and stays usable`, async ({
      api,
      paneCleanup,
      spawnAgent,
      pollMarkerInDom,
      ui,
    }) => {
      test.setTimeout(90_000);
      const inbox = await paneCleanup.createPane({ title: "pw-restart-inbox" });
      const warmMarker = `PWWARM${Date.now()}`;
      const resumeMarker = `PWRESUME${Date.now()}`;
      let agentPaneId: string | undefined;
      try {
        await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
        const spawned = await spawnAgent({
          agent: provider,
          role: "Worker",
          title: "pw-agent-restart",
          parent_pane_id: inbox.id,
          report_target_pane_id: inbox.id,
          skip_permissions: true,
          activate: true,
          cwd: tempSpawnCwd("pw-stub-agent-"),
          task: `STUB_ECHO ${warmMarker}`,
        });
        agentPaneId = spawned.pane_id;
        await showTerminalView(ui.page, agentPaneId);
        await pollMarkerInDom(`ack ${warmMarker}`, {
          paneId: agentPaneId,
          timeout: 60_000,
        });

        const sessionBefore = (await api.panes()).find(
          (p) => p.id === agentPaneId,
        ) as { agent_session_id?: string } | undefined;
        expect(
          sessionBefore?.agent_session_id,
          "session id must exist before restart",
        ).toBeTruthy();

        const restart = await api.post<{ restarting: boolean }>(
          `/panes/${agentPaneId}/agent/restart`,
          {},
        );
        expect(restart.restarting).toBe(true);

        const pane = await pollUntil(
          async () => {
            const found = (await api.panes()).find(
              (p) => p.id === agentPaneId,
            ) as
              | { agent_type?: string | null; agent_session_id?: string }
              | undefined;
            return found?.agent_type === provider ? found : undefined;
          },
          60_000,
          1_000,
          "agent pane survives restart",
        );
        expect(pane?.agent_type, "pane lost its agent after restart").toBe(
          provider,
        );

        await showTerminalView(ui.page, agentPaneId);
        await api.post(`/panes/${agentPaneId}/send`, {
          text: `STUB_ECHO ${resumeMarker}`,
          enter: true,
          raw: true,
        });
        await pollMarkerInDom(`ack ${resumeMarker}`, {
          paneId: agentPaneId,
          timeout: 60_000,
        });

        const after = (await api.panes()).find((p) => p.id === agentPaneId) as
          { agent_session_id?: string } | undefined;
        expect(
          after?.agent_session_id,
          "restart replaced the session instead of resuming",
        ).toBe(sessionBefore!.agent_session_id);
      } finally {
        if (agentPaneId) paneCleanup.track(agentPaneId);
      }
    });
  }
});
