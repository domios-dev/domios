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
  tabCleanup,
}) => {
  const parent = await tabCleanup.createTab({ title: "pw-report-parent" });
  const child = await tabCleanup.createTab({ title: "pw-report-child" });
  // 2-level hierarchy: a Worker's parent must be an Orchestrator.
  await api.post(`/tabs/${parent.id}/role`, { role: "Orchestrator" });
  await api.post(`/tabs/${child.id}/role`, {
    role: "Worker",
    parent_tab_id: parent.id,
  });

  const marker = `pw-report-${Date.now()}`;
  const submitted = await api.post<{ ok: boolean; report_id: string }>(
    `/tabs/${child.id}/report`,
    { message: marker },
  );
  expect(submitted.ok).toBe(true);
  expect(submitted.report_id).toBeTruthy();

  const reports = await api.get<unknown>(
    `/tabs/${parent.id}/reports?clear=true`,
  );
  const text = JSON.stringify(reports);
  expect(
    text,
    `parent inbox missing the report: ${text.slice(0, 300)}`,
  ).toContain(marker);
  expect(text, "report must carry the reporting tab id").toContain(child.id);
});

test("@smoke tab status endpoint answers for a live tab and 404s for a dead id", async ({
  api,
  tabCleanup,
}) => {
  const tab = await tabCleanup.createTab({ title: "pw-status-probe" });
  const status = await api.get<Record<string, unknown>>(
    `/tabs/${tab.id}/status`,
  );
  expect(typeof status).toBe("object");
  await api.closeTab(tab.id).catch(() => {});
  let code = 200;
  try {
    await api.get("/tabs/no-such-tab/status");
  } catch (e) {
    code = Number(/HTTP (\d+)/.exec(String(e))?.[1] ?? 0);
  }
  expect(code).toBe(404);
});

test("@smoke make-orchestrator rejects plain shell tabs", async ({
  api,
  tabCleanup,
}) => {
  const tab = await tabCleanup.createTab({ title: "pw-make-orch-shell" });
  let status = 0;
  try {
    await api.post(`/tabs/${tab.id}/make-orchestrator`, {});
  } catch (e) {
    status = Number(/HTTP (\d+)/.exec(String(e))?.[1] ?? 0);
  }
  expect(status).toBe(400);
});

test("@smoke make-orchestrator promotes agent tab and surfaces onboarding", async ({
  api,
  tabCleanup,
}) => {
  const tab = await tabCleanup.createTab({
    title: "pw-make-orch-agent",
    eager: true,
  });
  await api.post(`/tabs/${tab.id}/agent-type`, { agent_type: "claude" });

  const promoted = await api.post<{ ok: boolean }>(
    `/tabs/${tab.id}/make-orchestrator`,
    {},
  );
  expect(promoted.ok).toBe(true);

  const state = await api.get<{
    tabs: Record<string, { role?: string; can_spawn?: boolean }>;
  }>("/mux/state");
  const live = state.tabs[tab.id];
  expect(live?.role).toBe("Orchestrator");
  expect(live?.can_spawn).toBe(true);

  // ChatSubscriptionBody requires subscriptionId (camelCase); `{}` is rejected
  // with "missing field subscriptionId" before the handler runs.
  const chat = await api.post<{ messages?: { content?: string }[] }>(
    `/tabs/${tab.id}/chat-raw/start`,
    { subscriptionId: `pw-agents-${tab.id}` },
  );
  const greeting = JSON.stringify(chat.messages ?? []);
  expect(greeting).toContain("You are now the Orchestrator");
  expect(greeting).toContain("codeterm doc orchestration");
  expect(greeting).toContain(`codeterm agent spawn --parent ${tab.id}`);
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
      tabCleanup,
      spawnAgent,
    }) => {
      test.setTimeout(90_000);
      const inbox = await tabCleanup.createTab({ title: "pw-agent-inbox" });
      const marker = `PW-AGENT-OK-${Date.now()}`;
      let agentTabId: string | undefined;
      try {
        await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
        const spawned = await spawnAgent<{ tab_id: string; agent: string }>({
          agent: provider,
          role: "Worker",
          title: "pw-agent-golden",
          parent_tab_id: inbox.id,
          report_target_tab_id: inbox.id,
          skip_permissions: true,
          cwd: tempSpawnCwd("pw-stub-agent-"),
          task: `STUB_REPORT ${marker}`,
        });
        agentTabId = spawned.tab_id;
        expect(agentTabId).toBeTruthy();
        expect(spawned.agent).toBe(provider);

        const tab = (await api.tabs()).find((p) => p.id === agentTabId);
        expect(tab, "spawned agent tab missing from tab list").toBeDefined();
        expect(tab!.agent_type).toBe(provider);

        const reportText = await pollUntil(
          async () => {
            const reports = await api.get<unknown>(
              `/tabs/${inbox.id}/reports`,
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
          `/tabs/${agentTabId}/status`,
        );
        expect(status.role).toBe("Worker");
        expect(status.parent_tab_id).toBe(inbox.id);
        expect(status.agent_type).toBe(provider);
        const sid = await pollUntil(
          async () => {
            const tabs = await api.tabs();
            return (
              tabs.find((p) => p.id === agentTabId) as
                { agent_session_id?: string } | undefined
            )?.agent_session_id;
          },
          30_000,
          500,
          "session id materializes",
        );
        expect(sid ?? "").not.toBe("");
      } finally {
        if (agentTabId) tabCleanup.track(agentTabId);
      }
    });

    test(`@golden ${provider}: multi-turn + tool round-trip observed in the live terminal DOM`, async ({
      api,
      tabCleanup,
      spawnAgent,
      pollMarkerInDom,
      ui,
    }) => {
      test.setTimeout(90_000);
      const inbox = await tabCleanup.createTab({ title: "pw-mt-inbox" });
      const toolMarker = `PWTOOL${Date.now()}`;
      const turn2Marker = `PWTURN2${Date.now()}`;
      let agentTabId: string | undefined;
      try {
        await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
        const spawned = await spawnAgent({
          agent: provider,
          role: "Worker",
          title: "pw-agent-live",
          parent_tab_id: inbox.id,
          report_target_tab_id: inbox.id,
          skip_permissions: true,
          activate: true,
          cwd: tempSpawnCwd("pw-stub-agent-"),
          task: `STUB_TOOL ${toolMarker}`,
        });
        agentTabId = spawned.tab_id;
        await showTerminalView(ui.page, agentTabId);
        const toolReply = `tool round-trip done ${toolMarker}`;
        await pollMarkerInDom(toolReply, {
          tabId: agentTabId,
          timeout: 60_000,
        });
        const toolText = await api.get<{ text?: string } | string>(
          `/tabs/${agentTabId}/text`,
        );
        const toolBuffer =
          typeof toolText === "string"
            ? toolText
            : (toolText.text ?? JSON.stringify(toolText));
        expect(
          toolBuffer,
          "stub tool reply missing from tab buffer",
        ).toContain(toolReply);

        await api.post(`/tabs/${agentTabId}/send`, {
          text: `STUB_ECHO ${turn2Marker}`,
          enter: true,
          raw: true,
        });
        const turn2Reply = `ack ${turn2Marker}`;
        await pollMarkerInDom(turn2Reply, {
          tabId: agentTabId,
          timeout: 60_000,
        });
        const turn2Buffer = await pollUntil(
          async () => {
            const turn2Text = await api.get<{ text?: string } | string>(
              `/tabs/${agentTabId}/text`,
            );
            const buffer =
              typeof turn2Text === "string"
                ? turn2Text
                : (turn2Text.text ?? JSON.stringify(turn2Text));
            return buffer.includes(turn2Reply) ? buffer : undefined;
          },
          10_000,
          250,
          "second stub reply reaches tab text buffer",
        );
        expect(
          turn2Buffer,
          "second stub reply missing from tab buffer",
        ).toContain(turn2Reply);
      } finally {
        if (agentTabId) tabCleanup.track(agentTabId);
      }
    });

    test(`@golden ${provider}: agent response read from the rendered terminal DOM`, async ({
      api,
      tabCleanup,
      spawnAgent,
      pollMarkerInDom,
      ui,
    }) => {
      test.setTimeout(90_000);
      const inbox = await tabCleanup.createTab({ title: "pw-dom-inbox" });
      const marker = `PWDOM${Date.now()}`;
      let agentTabId: string | undefined;
      try {
        await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
        const spawned = await spawnAgent({
          agent: provider,
          role: "Worker",
          title: "pw-agent-dom",
          parent_tab_id: inbox.id,
          report_target_tab_id: inbox.id,
          skip_permissions: true,
          activate: true,
          cwd: tempSpawnCwd("pw-stub-agent-"),
          task: `STUB_ECHO ${marker}`,
        });
        agentTabId = spawned.tab_id;
        await showTerminalView(ui.page, agentTabId);
        const reply = `ack ${marker}`;
        await pollMarkerInDom(reply, { tabId: agentTabId, timeout: 60_000 });
        const text = await api.get<{ text?: string } | string>(
          `/tabs/${agentTabId}/text`,
        );
        const buffer =
          typeof text === "string" ? text : (text.text ?? JSON.stringify(text));
        expect(
          buffer,
          "PTY buffer missing the stub reply the DOM showed",
        ).toContain(reply);
      } finally {
        if (agentTabId) tabCleanup.track(agentTabId);
      }
    });

    test(`@golden ${provider}: agent restart mid-session resumes and stays usable`, async ({
      api,
      tabCleanup,
      spawnAgent,
      pollMarkerInDom,
      ui,
    }) => {
      test.setTimeout(90_000);
      const inbox = await tabCleanup.createTab({ title: "pw-restart-inbox" });
      const warmMarker = `PWWARM${Date.now()}`;
      const resumeMarker = `PWRESUME${Date.now()}`;
      let agentTabId: string | undefined;
      try {
        await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
        const spawned = await spawnAgent({
          agent: provider,
          role: "Worker",
          title: "pw-agent-restart",
          parent_tab_id: inbox.id,
          report_target_tab_id: inbox.id,
          skip_permissions: true,
          activate: true,
          cwd: tempSpawnCwd("pw-stub-agent-"),
          task: `STUB_ECHO ${warmMarker}`,
        });
        agentTabId = spawned.tab_id;
        await showTerminalView(ui.page, agentTabId);
        await pollMarkerInDom(`ack ${warmMarker}`, {
          tabId: agentTabId,
          timeout: 60_000,
        });

        const sessionBefore = (await api.tabs()).find(
          (p) => p.id === agentTabId,
        ) as { agent_session_id?: string } | undefined;
        expect(
          sessionBefore?.agent_session_id,
          "session id must exist before restart",
        ).toBeTruthy();

        const restart = await api.post<{ restarting: boolean }>(
          `/tabs/${agentTabId}/agent/restart`,
          {},
        );
        expect(restart.restarting).toBe(true);

        const tab = await pollUntil(
          async () => {
            const found = (await api.tabs()).find(
              (p) => p.id === agentTabId,
            ) as
              | { agent_type?: string | null; agent_session_id?: string }
              | undefined;
            return found?.agent_type === provider ? found : undefined;
          },
          60_000,
          1_000,
          "agent tab survives restart",
        );
        expect(tab?.agent_type, "tab lost its agent after restart").toBe(
          provider,
        );

        await showTerminalView(ui.page, agentTabId);
        await api.post(`/tabs/${agentTabId}/send`, {
          text: `STUB_ECHO ${resumeMarker}`,
          enter: true,
          raw: true,
        });
        await pollMarkerInDom(`ack ${resumeMarker}`, {
          tabId: agentTabId,
          timeout: 60_000,
        });

        const after = (await api.tabs()).find((p) => p.id === agentTabId) as
          { agent_session_id?: string } | undefined;
        expect(
          after?.agent_session_id,
          "restart replaced the session instead of resuming",
        ).toBe(sessionBefore!.agent_session_id);
      } finally {
        if (agentTabId) tabCleanup.track(agentTabId);
      }
    });
  }
});
