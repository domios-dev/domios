/**
 * WHAT: Send Chat UI payloads across live PTY providers under load and restore.
 * WHY: Provider-owned session receipts expose lost, duplicated, reordered, or stuck sends.
 * ORACLE: Exact tab-session turns, final confirmed ledger states, Chat bubbles, and body hashes.
 */
import { randomUUID } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import { isDevTarget } from "../../helpers/dev-instance";
import {
  HEAVY_OUTPUT_BYTES,
  HEAVY_TURNS,
  LARGE_SIZES,
  TabEvents,
  TEST_TIMEOUT_MS,
  TURN_TIMEOUT_MS,
  agentState,
  assertFinalStatuses,
  auditTranscript,
  busyPrompt,
  completeCase,
  createBusyGate,
  createWorkspace,
  disposeAgent,
  makeLargePayload,
  openAgent,
  readChat,
  recordMetrics,
  releaseBusyGate,
  removeBusyGate,
  repeatCount,
  submitBatch,
  submitLarge,
  submitTextWithoutAudit,
  toolOutputPrompt,
  uniqueMarker,
  verifyLargeReceipt,
  waitDelivered,
  waitForAndAuditRecords,
  waitForFile,
  waitForIdle,
  waitForWorking,
} from "./chat-send-matrix.live.helpers";
import type {
  AgentUi,
  BusyGate,
  ChatSendStatus,
  ProviderMetrics,
  SendRecord,
} from "./chat-send-matrix.live.helpers";

test.describe.configure({ retries: 0 });

test("@live Chat UI sends are exact-once across available PTY providers", async ({
  api,
  ui,
  spawnAgent,
}, testInfo) => {
  test.setTimeout(TEST_TIMEOUT_MS);
  const repeats = repeatCount();
  const runId = randomUUID().replace(/-/g, "");
  const workspaceName = `pw-send-${runId}`;
  const providers = (await api.providers()).filter((provider) => provider.transport === "pty");
  expect(providers.length, "/agents/providers reports PTY providers").toBeGreaterThan(0);

  const metrics = new Map<string, ProviderMetrics>();
  for (const provider of providers) {
    const entry = { provider, records: [] } as ProviderMetrics;
    if (provider.availability === "unavailable") {
      entry.skipped = "instance capability API reports unavailable";
      console.log(`[chat-send-matrix-skip] ${JSON.stringify(recordMetrics(entry))}`);
      testInfo.annotations.push({
        type: "provider-skip",
        description: `${provider.display_name ?? provider.id}: unavailable according to provider capability API`,
      });
    } else {
      expect(provider.supports_chat_ui, `${provider.display_name ?? provider.id} supports Chat UI`).toBe(true);
    }
    metrics.set(provider.id, entry);
  }

  const runnable = providers.filter((provider) => provider.availability !== "unavailable");
  expect(runnable.length, "at least one PTY provider is available or capability-unknown").toBeGreaterThan(0);
  const ownedTabs: string[] = [];
  const openStreams = new Set<TabEvents>();
  const busyGates = new Set<BusyGate>();
  let workspaceId: string | undefined;

  try {
    workspaceId = await createWorkspace(api, workspaceName);

    for (const [providerIndex, provider] of runnable.entries()) {
      const entry = metrics.get(provider.id)!;

      const fresh = await openAgent(
        api,
        ui,
        spawnAgent,
        provider,
        workspaceId,
        `pw-send-${providerIndex}-fresh`,
      );
      ownedTabs.push(fresh.tabId);
      openStreams.add(fresh.events);
      const freshRecords = await submitBatch(
        fresh,
        provider,
        entry,
        Array.from({ length: repeats }, (_, index) => {
          const marker = uniqueMarker(runId, provider, "FRESH", index);
          return { text: `Receive this exact fresh-tab marker once: ${marker}`, marker };
        }),
      );
      await waitForAndAuditRecords(api, fresh, freshRecords, entry);
      await disposeAgent(api, fresh);
      openStreams.delete(fresh.events);

      const busy = await openAgent(
        api,
        ui,
        spawnAgent,
        provider,
        workspaceId,
        `pw-send-${providerIndex}-busy`,
      );
      ownedTabs.push(busy.tabId);
      openStreams.add(busy.events);
      const firstMarker = uniqueMarker(runId, provider, "BUSY_WORK", 0);
      const busyGate = createBusyGate(busy, firstMarker);
      busyGates.add(busyGate);
      const first = await submitTextWithoutAudit(
        busy,
        provider,
        entry,
        busyPrompt(busyGate),
        firstMarker,
      );
      await waitDelivered(busy, first);
      await waitForFile(busyGate.readyPath, TURN_TIMEOUT_MS, "busy command readiness");
      await expect(agentState(busy), "first long prompt is actively running").toHaveAttribute(
        "data-agent-state",
        "working",
        { timeout: TURN_TIMEOUT_MS },
      );
      const busyRecords = [first, ...(await submitBatch(
        busy,
        provider,
        entry,
        Array.from({ length: repeats }, (_, index) => {
          const marker = uniqueMarker(runId, provider, "BUSY", index);
          return { text: `Queue this marker behind current work and preserve order: ${marker}`, marker };
        }),
        true,
      ))];
      await Promise.all(busyRecords.map((record) => waitDelivered(busy, record)));
      const beforeBusyFinish = await readChat(api, busy.tabId);
      expect(
        beforeBusyFinish.messages.some(
          (message) => message.type === "tool_result" && message.content.includes(firstMarker),
        ),
        "busy follow-up receipts precede the long prompt tool result",
      ).toBe(false);
      await expect(agentState(busy), "busy send receipts arrive before the first long turn completes").toHaveAttribute(
        "data-agent-state",
        "working",
        { timeout: 15_000 },
      );
      await assertFinalStatuses(api, busy, busyRecords);
      await auditTranscript(api, busy, busyRecords, entry);
      releaseBusyGate(busyGate);
      await waitForIdle(busy, api, "busy queued sends");
      const completedBusy = await readChat(api, busy.tabId);
      expect(
        completedBusy.messages.filter(
          (message) => message.type === "tool_result" && message.content.includes(firstMarker),
        ),
        "released busy command completes once",
      ).toHaveLength(1);
      removeBusyGate(busyGate);
      busyGates.delete(busyGate);
      await disposeAgent(api, busy);
      openStreams.delete(busy.events);

      const rapid = await openAgent(
        api,
        ui,
        spawnAgent,
        provider,
        workspaceId,
        `pw-send-${providerIndex}-rapid`,
      );
      ownedTabs.push(rapid.tabId);
      openStreams.add(rapid.events);
      const rapidRecords: SendRecord[] = [];
      for (let index = 0; index < repeats; index++) {
        const a = uniqueMarker(runId, provider, "RAPID_A", index);
        const b = uniqueMarker(runId, provider, "RAPID_B", index);
        const pair = [
          { text: `Rapid send A in pair ${index}: ${a}`, marker: a },
          { text: `Rapid send B in pair ${index}: ${b}`, marker: b },
        ];
        const pairRecords = await submitBatch(rapid, provider, entry, pair);
        rapidRecords.push(...pairRecords);
        await waitForAndAuditRecords(api, rapid, pairRecords, entry);
      }
      await assertFinalStatuses(api, rapid, rapidRecords);
      await auditTranscript(api, rapid, rapidRecords, entry);
      await disposeAgent(api, rapid);
      openStreams.delete(rapid.events);

      const large = await openAgent(
        api,
        ui,
        spawnAgent,
        provider,
        workspaceId,
        `pw-send-${providerIndex}-large`,
      );
      ownedTabs.push(large.tabId);
      openStreams.add(large.events);
      for (const size of LARGE_SIZES) {
        for (let index = 0; index < repeats; index++) {
          const payload = makeLargePayload(size, runId, provider, index);
          const record = await submitLarge(api, large, provider, entry, ui, payload);
          await waitForWorking(large, `large paste ${size} bytes ${index}`);
          await waitForIdle(large, api, `large paste ${size} bytes ${index}`);
          await verifyLargeReceipt(api, large, provider, entry, record, payload, runId);
        }
      }
      await auditTranscript(api, large, entry.records.filter((record) => record.tabId === large.tabId), entry);
      await disposeAgent(api, large);
      openStreams.delete(large.events);

      const heavy = await openAgent(
        api,
        ui,
        spawnAgent,
        provider,
        workspaceId,
        `pw-send-${providerIndex}-heavy`,
        "Orchestrator",
      );
      ownedTabs.push(heavy.tabId);
      openStreams.add(heavy.events);
      const historyRecords: SendRecord[] = [];
      for (let turn = 0; turn < HEAVY_TURNS; turn++) {
        const marker = uniqueMarker(runId, provider, "HEAVY_TOOL", turn);
        const prompt = toolOutputPrompt(marker, HEAVY_OUTPUT_BYTES);
        const record = await submitTextWithoutAudit(heavy, provider, entry, prompt, marker);
        historyRecords.push(record);
        await waitDelivered(heavy, record);
        await waitForWorking(heavy, `heavy history turn ${turn}`);
        await waitForIdle(heavy, api, `heavy history turn ${turn}`);
        const snapshot = await readChat(api, heavy.tabId);
        const outputs = snapshot.messages.filter(
          (message) => message.type === "tool_result" && message.content.includes(marker),
        );
        expect(outputs.length, `heavy session stores tool output ${turn} once`).toBe(1);
        expect(outputs[0].content).toContain(`${marker}_END`);
        expect(outputs[0].content.length).toBeGreaterThanOrEqual(HEAVY_OUTPUT_BYTES);
      }
      await completeCase(api, heavy, historyRecords, entry);

      const heavyRepeat = await submitBatch(
        heavy,
        provider,
        entry,
        Array.from({ length: repeats }, (_, index) => {
          const marker = uniqueMarker(runId, provider, "HEAVY_REPEAT", index);
          return { text: `Long-context repeat ${index}: ${marker}`, marker };
        }),
      );
      await waitForAndAuditRecords(api, heavy, heavyRepeat, entry);

      const heavyBusyMarker = uniqueMarker(runId, provider, "HEAVY_BUSY_WORK", 0);
      const heavyBusyGate = createBusyGate(heavy, heavyBusyMarker);
      busyGates.add(heavyBusyGate);
      const heavyBusy = await submitTextWithoutAudit(
        heavy,
        provider,
        entry,
        busyPrompt(heavyBusyGate),
        heavyBusyMarker,
      );
      await waitDelivered(heavy, heavyBusy);
      await waitForFile(heavyBusyGate.readyPath, TURN_TIMEOUT_MS, "heavy-context busy command readiness");
      await waitForWorking(heavy, "heavy-context busy turn");
      const heavyBusyRecords = await submitBatch(
        heavy,
        provider,
        entry,
        Array.from({ length: repeats }, (_, index) => {
          const marker = uniqueMarker(runId, provider, "HEAVY_BUSY", index);
          return { text: `Busy heavy-context send ${index}: ${marker}`, marker };
        }),
        true,
      );
      const busyPayload = makeLargePayload(LARGE_SIZES[1], runId, provider, 0, "HEAVY_BUSY_LARGE");
      const busyLarge = await submitLarge(api, heavy, provider, entry, ui, busyPayload);
      await waitDelivered(heavy, heavyBusy);
      await Promise.all(heavyBusyRecords.map((record) => waitDelivered(heavy, record)));
      const beforeHeavyBusyFinish = await readChat(api, heavy.tabId);
      expect(
        beforeHeavyBusyFinish.messages.some(
          (message) => message.type === "tool_result" && message.content.includes(heavyBusyMarker),
        ),
        "heavy-context follow-up receipts precede the long prompt tool result",
      ).toBe(false);
      await expect(agentState(heavy), "heavy-context sends are received while work is active").toHaveAttribute(
        "data-agent-state",
        "working",
        { timeout: 15_000 },
      );
      await assertFinalStatuses(api, heavy, [heavyBusy, ...heavyBusyRecords, busyLarge]);
      await auditTranscript(api, heavy, [heavyBusy, ...heavyBusyRecords, busyLarge], entry);
      releaseBusyGate(heavyBusyGate);
      await waitForIdle(heavy, api, "heavy-context busy queue");
      const completedHeavyBusy = await readChat(api, heavy.tabId);
      expect(
        completedHeavyBusy.messages.filter(
          (message) => message.type === "tool_result" && message.content.includes(heavyBusyMarker),
        ),
        "released heavy-context command completes once",
      ).toHaveLength(1);
      removeBusyGate(heavyBusyGate);
      busyGates.delete(heavyBusyGate);
      await verifyLargeReceipt(api, heavy, provider, entry, busyLarge, busyPayload, runId);
      await auditTranscript(
        api,
        heavy,
        entry.records.filter((record) => record.tabId === heavy.tabId),
        entry,
      );

      if (isDevTarget() && process.env.PW_DEV_RESTORE !== "1") {
        console.log(
          `[chat-send-matrix-restore-skip] ${provider.display_name ?? provider.id}: PW_TARGET=dev requires PW_DEV_RESTORE=1`,
        );
        testInfo.annotations.push({
          type: "restore-skip",
          description: `${provider.display_name ?? provider.id}: DEV restart is opt-in via PW_DEV_RESTORE=1`,
        });
      } else {
        await heavy.events.close();
        openStreams.delete(heavy.events);
        await api.post("/session/save");
        const { killDevConsole, ensureDevConsole } = await import("../../helpers/dev-console");
        killDevConsole();
        await ensureDevConsole();
        await ui.reconnect(true);

        const restoredTab = (await api.tabs()).find((tab) => tab.id === heavy.tabId);
        expect(restoredTab, "restore keeps the owned tab record").toBeTruthy();
        expect(restoredTab!.agent_type, "restore keeps this provider identity").toBe(provider.id);
        await api.focusWorkspace(workspaceId);
        await api.focusTab(heavy.tabId);
        const restoredRow = ui.page.locator(
          `[data-testid="tab-item"][data-tab-id="${heavy.tabId}"]`,
        );
        await expect(restoredRow, "restored tab row is visible").toBeVisible({ timeout: 45_000 });
        await restoredRow.click();
        const restoredChat = ui.page.locator(
          `[data-testid="tab-session-chat"][data-tab-id="${heavy.tabId}"]`,
        );
        await expect(restoredChat, "restored Chat UI is mounted").toBeVisible({ timeout: 45_000 });
        const restoredComposer = restoredChat.getByRole("textbox", { name: "Message composer" });
        await expect(restoredComposer, "restored Chat composer is available").toBeVisible({
          timeout: 45_000,
        });
        const restoredStatus = await api.get<{
          status: string;
          agent_type?: string | null;
          agent_session_id?: string | null;
        }>(`/tabs/${encodeURIComponent(heavy.tabId)}/status`);
        expect(restoredStatus.status, "restored agent reaches agent_ready / Idle").toBe("Idle");
        expect(restoredStatus.agent_type).toBe(provider.id);
        expect(restoredStatus.agent_session_id, "restored agent has its own bound session").toBeTruthy();

        const restoredEvents = await TabEvents.open(api, heavy.tabId);
        openStreams.add(restoredEvents);
        const statuses = await api.get<ChatSendStatus[]>(
          `/tabs/${encodeURIComponent(heavy.tabId)}/chat-send`,
        );
        const restoredAgent: AgentUi = {
          tabId: heavy.tabId,
          chat: restoredChat,
          composer: restoredComposer,
          events: restoredEvents,
          nextSendSequence: Math.max(0, ...statuses.map((status) => status.sequence)),
        };
        const restoredRecords = await submitBatch(
          restoredAgent,
          provider,
          entry,
          Array.from({ length: repeats }, (_, index) => {
            const marker = uniqueMarker(runId, provider, "RESTORED", index);
            return { text: `After restore, receive this marker once: ${marker}`, marker };
          }),
        );
        await waitForAndAuditRecords(api, restoredAgent, restoredRecords, entry);
        await auditTranscript(
          api,
          restoredAgent,
          entry.records.filter((record) => record.tabId === heavy.tabId),
          entry,
        );
        await disposeAgent(api, restoredAgent);
        openStreams.delete(restoredEvents);
      }

      if (openStreams.has(heavy.events)) {
        await disposeAgent(api, heavy);
        openStreams.delete(heavy.events);
      } else {
        await api.closeTab(heavy.tabId).catch(() => {});
      }
    }

    const soakProvider = runnable[0];
    const soakMetrics = metrics.get(soakProvider.id)!;
    const soak = await openAgent(
      api,
      ui,
      spawnAgent,
      soakProvider,
      workspaceId,
      "pw-send-100-soak",
    );
    ownedTabs.push(soak.tabId);
    openStreams.add(soak.events);
    const soakRecords = await submitBatch(
      soak,
      soakProvider,
      soakMetrics,
      Array.from({ length: 100 }, (_, index) => {
        const marker = uniqueMarker(runId, soakProvider, "SOAK_100", index);
        return { text: `100-send soak ${index}: ${marker}`, marker };
      }),
    );
    await waitForAndAuditRecords(api, soak, soakRecords, soakMetrics);
    await disposeAgent(api, soak);
    openStreams.delete(soak.events);

    const parallelAssignments = Array.from({ length: 10 }, (_, index) => runnable[index % runnable.length]);
    const parallelAgents = await Promise.all(
      parallelAssignments.map((provider, index) =>
        openAgent(
          api,
          ui,
          spawnAgent,
          provider,
          workspaceId!,
          `pw-send-parallel-${index}`,
        ),
      ),
    );
    parallelAgents.forEach((agent) => {
      ownedTabs.push(agent.tabId);
      openStreams.add(agent.events);
    });
    const parallelRecords = parallelAgents.map(() => [] as SendRecord[]);
    for (let round = 0; round < 10; round++) {
      for (let index = 0; index < parallelAgents.length; index++) {
        const provider = parallelAssignments[index];
        const marker = uniqueMarker(runId, provider, `PARALLEL_${index}`, round);
        const records = await submitBatch(
          parallelAgents[index],
          provider,
          metrics.get(provider.id)!,
          [{ text: `Parallel tab ${index}, send ${round}: ${marker}`, marker }],
        );
        parallelRecords[index].push(...records);
      }
    }
    await Promise.all(
      parallelAgents.map((agent, index) =>
        waitForAndAuditRecords(
          api,
          agent,
          parallelRecords[index],
          metrics.get(parallelAssignments[index].id)!,
        ),
      ),
    );
    for (const agent of parallelAgents) {
      await disposeAgent(api, agent);
      openStreams.delete(agent.events);
    }

    const summaries = providers.map((provider) => recordMetrics(metrics.get(provider.id)!));
    console.log(`[chat-send-matrix-summary] ${JSON.stringify(summaries)}`);
    await testInfo.attach("chat-send-matrix-summary.json", {
      body: Buffer.from(JSON.stringify(summaries, null, 2)),
      contentType: "application/json",
    });
    expect(summaries.reduce((total, item) => total + Number(item.duplicated), 0)).toBe(0);
    expect(summaries.reduce((total, item) => total + Number(item.lost), 0)).toBe(0);
    for (const summary of summaries) {
      if (summary.skipped) continue;
      expect(summary.received, `${summary.provider} receives every submitted send`).toBe(summary.sent);
    }
  } finally {
    for (const gate of busyGates) releaseBusyGate(gate);
    await Promise.all(
      [...busyGates].map((gate) => waitForIdle(gate.agent, api, "busy command cleanup").catch(() => {})),
    );
    for (const tabId of ownedTabs.reverse()) await api.closeTab(tabId).catch(() => {});
    for (const gate of busyGates) removeBusyGate(gate);
    busyGates.clear();
    for (const events of openStreams) await events.close();
    if (workspaceId) {
      await api.closeWorkspace(workspaceId).catch(() => {});
    } else {
      const createdWorkspace = (await api.workspaces().catch(() => [])).find(
        (workspace) => workspace.name === workspaceName,
      );
      if (createdWorkspace) await api.closeWorkspace(createdWorkspace.id).catch(() => {});
    }
    const summaries = providers.map((provider) => recordMetrics(metrics.get(provider.id)!));
    const summaryPath = path.join(
      process.cwd(),
      "tests",
      "playwright",
      "artifacts",
      `chat-send-matrix-${runId}.json`,
    );
    mkdirSync(path.dirname(summaryPath), { recursive: true });
    writeFileSync(summaryPath, `${JSON.stringify(summaries, null, 2)}\n`, "utf8");
    console.log(`[chat-send-matrix-summary-file] ${summaryPath}`);
    console.log(`[chat-send-matrix-final] ${JSON.stringify(summaries)}`);
  }
});
