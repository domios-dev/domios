/**
 * WHAT: real-provider end-to-end agent flows (live LLM), run on their own cadence.
 * WHY: the golden variants run on the stub transport; these keep true
 *      provider behavior covered outside the golden gate.
 * ORACLE: identical to the converted golden tests — report inbox, DOM
 *      markers, restart/resume session identity.
 */
import { expect, test, type SpawnAgent } from "../../helpers/fixtures";
import { pollUntil } from "../../helpers/poll";
import { type ApiClient } from "../../helpers/api-client";

test("@live real agent spawn → works → reports back (structural asserts)", async ({ api, tabCleanup, spawnAgent }) => {
  // Real model latency + CLI startup; bounded, not unbounded.
  test.setTimeout(300_000);

  const inbox = await tabCleanup.createTab({ title: "pw-agent-inbox" });
  const marker = `PW-AGENT-OK-${Date.now()}`;
  let agentTabId: string | undefined;

  try {
    // 2-level hierarchy: a Worker must hang off an Orchestrator parent.
    await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await spawnAgent<{
      tab_id: string;
      agent: string;
      role?: string;
    }>({
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-agent-golden",
      parent_tab_id: inbox.id,
      report_target_tab_id: inbox.id,
      skip_permissions: true,
      task:
        `Run this exact command and then stop: codeterm agent report "${marker}". ` +
        "Do not do anything else. Do not explain.",
    });
    agentTabId = spawned.tab_id;
    expect(agentTabId).toBeTruthy();
    expect(spawned.agent).toBe("claude");

    // Structural: the tab exists and is typed as a claude agent tab.
    const tab = (await api.tabs()).find((p) => p.id === agentTabId);
    expect(tab, "spawned agent tab missing from tab list").toBeDefined();
    expect(tab!.agent_type).toBe("claude");

    const reportText = await pollUntil(async () => {
      const reports = await api.get<unknown>(`/tabs/${inbox.id}/reports`);
      const text = JSON.stringify(reports);
      return text.includes(marker) ? text : undefined;
    }, 240_000, 3_000, "agent report reaches inbox");
    expect(reportText, "agent report with marker never arrived").toContain(marker);

    // Session detection is async but should have landed by report time.
    const status = await api.get<Record<string, unknown>>(`/tabs/${agentTabId}/status`);
    expect(status.role).toBe("Worker");
    expect(status.parent_tab_id).toBe(inbox.id);
    expect(status.agent_type).toBe("claude");
    expect(status.agent_session_id ?? "").not.toBe("");
  } finally {
    if (agentTabId) tabCleanup.track(agentTabId);
  }
});

async function spawnWorker(spawnAgent: SpawnAgent, inboxId: string, task: string): Promise<string> {
  const spawned = await spawnAgent({
    agent: "claude",
    model: "sonnet",
    role: "Worker",
    title: "pw-agent-live",
    parent_tab_id: inboxId,
    report_target_tab_id: inboxId,
    skip_permissions: true,
    activate: true,
    task,
  });
  return spawned.tab_id;
}

test("@live multi-turn + tool round-trip observed in the live terminal DOM", async ({ api, tabCleanup, spawnAgent, pollMarkerInDom }) => {
  test.setTimeout(420_000);

  const inbox = await tabCleanup.createTab({ title: "pw-mt-inbox" });
  const toolMarker = `PW-TOOL-${Date.now()}`;
  const toolLeft = toolMarker.slice(0, Math.floor(toolMarker.length / 2));
  const toolRight = toolMarker.slice(Math.floor(toolMarker.length / 2));
  const turn2Marker = `PW-TURN2-${Date.now()}`;
  let agentTabId: string | undefined;

  try {
    await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
    agentTabId = await spawnWorker(
      spawnAgent,
      inbox.id,
      `Run exactly this command and nothing else, then stop: node -e "console.log('${toolLeft}' + '${toolRight}')"`,
    );
    await pollMarkerInDom(toolMarker);
    const toolText = await api.get<{ text?: string } | string>(`/tabs/${agentTabId}/text`);
    const toolBuffer = typeof toolText === "string" ? toolText : (toolText.text ?? JSON.stringify(toolText));
    expect(toolBuffer, "tool command marker missing from tab buffer").toContain(toolMarker);

    // Same tab, same session — a real second turn.
    await api.post(`/tabs/${agentTabId}/send`, {
      text: `Reply with a single line containing exactly ${turn2Marker} and nothing else.`,
      enter: true,
    });
    await pollMarkerInDom(turn2Marker);
    const turn2Text = await api.get<{ text?: string } | string>(`/tabs/${agentTabId}/text`);
    const turn2Buffer = typeof turn2Text === "string" ? turn2Text : (turn2Text.text ?? JSON.stringify(turn2Text));
    expect(turn2Buffer, "second turn marker missing from tab buffer").toContain(turn2Marker);
  } finally {
    if (agentTabId) tabCleanup.track(agentTabId);
  }
});

test("@live agent restart mid-session resumes and stays usable", async ({ api, tabCleanup, spawnAgent, pollMarkerInDom }) => {
  test.setTimeout(420_000);

  const inbox = await tabCleanup.createTab({ title: "pw-restart-inbox" });
  const warmMarker = `PW-WARM-${Date.now()}`;
  const resumeMarker = `PW-RESUME-${Date.now()}`;
  let agentTabId: string | undefined;

  try {
    await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
    agentTabId = await spawnWorker(
      spawnAgent,
      inbox.id,
      `Reply with a single line containing exactly ${warmMarker} and nothing else.`,
    );
    await pollMarkerInDom(warmMarker);

    const sessionBefore = (await api.tabs()).find((p) => p.id === agentTabId) as
      | { agent_session_id?: string }
      | undefined;

    // Graceful restart: /exit + auto-resume of the same session.
    const restart = await api.post<{ restarting: boolean; session_id?: string }>(
      `/tabs/${agentTabId}/agent/restart`,
      {},
    );
    expect(restart.restarting).toBe(true);

    // The tab must survive the restart and stay a claude agent tab.
    const tab = await pollUntil(async () => {
      const found = (await api.tabs()).find((p) => p.id === agentTabId) as
        | { agent_type?: string | null; agent_session_id?: string }
        | undefined;
      return found?.agent_type === "claude" ? found : undefined;
    }, 120_000, 2_000, "agent tab survives restart");
    expect(tab?.agent_type, "tab lost its agent after restart").toBe("claude");

    // Usability after restart is the real bar: a fresh prompt round-trips.
    await pollUntil(async () => {
      const status = await api.get<Record<string, unknown>>(`/tabs/${agentTabId}/status`);
      return status.agent_session_id || status.agent_type === "claude";
    }, 120_000, 2_000, "restarted agent is input-ready");
    await api.post(`/tabs/${agentTabId}/send`, {
      text: `Reply with a single line containing exactly ${resumeMarker} and nothing else.`,
      enter: true,
    });
    await pollMarkerInDom(resumeMarker);

    // known before the restart.
    if (sessionBefore?.agent_session_id) {
      const after = (await api.tabs()).find((p) => p.id === agentTabId) as
        | { agent_session_id?: string }
        | undefined;
      expect(after?.agent_session_id, "restart replaced the session instead of resuming").toBe(
        sessionBefore.agent_session_id,
      );
    }
  } finally {
    if (agentTabId) tabCleanup.track(agentTabId);
  }
});

test("@live fully live: agent response read from the rendered terminal DOM", async ({ api, tabCleanup, spawnAgent, pollMarkerInDom }) => {
  test.setTimeout(300_000);

  const inbox = await tabCleanup.createTab({ title: "pw-dom-inbox" });
  const marker = `PW-DOM-${Date.now()}`;
  let agentTabId: string | undefined;

  try {
    await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await spawnAgent({
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-agent-dom",
      parent_tab_id: inbox.id,
      report_target_tab_id: inbox.id,
      skip_permissions: true,
      activate: true,
      task: `Reply with a single line containing exactly ${marker} and nothing else.`,
    });
    agentTabId = spawned.tab_id;

    // tab's accessibility tree mirrors the viewport text.
    await pollMarkerInDom(marker);

    // daemon-side buffer must agree.
    const text = await api.get<{ text?: string } | string>(`/tabs/${agentTabId}/text`);
    const buffer = typeof text === "string" ? text : (text.text ?? JSON.stringify(text));
    expect(buffer, "PTY buffer missing the marker the DOM showed").toContain(marker);
  } finally {
    if (agentTabId) tabCleanup.track(agentTabId);
  }
});

// ── cross-tab chat: real agents obeying passive-fixture prompts ──

interface ChatMsg {
  role?: string;
  type?: string;
  content?: string;
}

async function chatContains(api: ApiClient, tabId: string, needle: string): Promise<ChatMsg | undefined> {
  const res = await api
    .get<{ messages: ChatMsg[] }>(`/tabs/${tabId}/chat`)
    .catch(() => ({ messages: [] as ChatMsg[] }));
  return res.messages.find((m: ChatMsg) => (m.content ?? "").includes(needle));
}

async function waitChatContains(api: ApiClient, tabId: string, needle: string, timeout = 120_000): Promise<ChatMsg> {
  const msg = await pollUntil(
    () => chatContains(api, tabId, needle),
    timeout,
    2_000,
    `tab ${tabId} chat receives "${needle}"`,
  );
  if (!msg) throw new Error(`tab ${tabId} chat never received "${needle}"`);
  return msg;
}

async function spawnLiveChatAgent(api: ApiClient, spawnAgent: SpawnAgent, title: string): Promise<string> {
  const spawned = await spawnAgent({
    agent: "claude",
    model: "sonnet",
    title,
    skip_permissions: true,
    activate: true,
    task:
      "You are a passive test fixture. Reply with the single word READY now. " +
      "For any later message, reply with only the word ACK. " +
      "Never run shell commands and never run any codeterm command.",
  });
  await api.post(`/tabs/${spawned.tab_id}/chat-view`, { enabled: true });
  return spawned.tab_id;
}

test("@live two chat tabs exchange a cross-tab message, origin rendered", async ({ api, ui, spawnAgent }) => {
  // Two agent boots + a turn each. Bounded for real model + CLI latency.
  test.setTimeout(420_000);

  const stamp = Date.now();
  const aToB = `PING-${stamp}`;
  const bToA = `PONG-${stamp}`;
  const [tabA, tabB] = await Promise.all([
    spawnLiveChatAgent(api, spawnAgent, `pw-A-${stamp}`),
    spawnLiveChatAgent(api, spawnAgent, `pw-B-${stamp}`),
  ]);
  await Promise.all([waitChatContains(api, tabA, "READY"), waitChatContains(api, tabB, "READY")]);

  await api.post(`/tabs/${tabB}/send`, { text: aToB, source_tab: tabA, enter: true });
  await api.post(`/tabs/${tabA}/send`, { text: bToA, source_tab: tabB, enter: true });

  const onB = await waitChatContains(api, tabB, aToB);
  const onA = await waitChatContains(api, tabA, bToA);
  expect(onB.content, "recipient B lost the cross-tab wire prefix").toContain("-=-codeterm:tab");
  expect(onA.content, "recipient A lost the cross-tab wire prefix").toContain("-=-codeterm:tab");
});
