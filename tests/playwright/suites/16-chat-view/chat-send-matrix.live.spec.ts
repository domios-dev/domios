/**
 * WHAT: Send Chat UI payloads across live PTY providers under load and restore.
 * WHY: Provider-owned session receipts expose lost, duplicated, reordered, or stuck sends.
 * ORACLE: Exact tab-session turns, final confirmed ledger states, Chat bubbles, and body hashes.
 */
import { randomUUID } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type SpawnAgent } from "../../helpers/fixtures";
import { isDevTarget } from "../../helpers/dev-instance";
import {
  HEAVY_OUTPUT_BYTES,
  HEAVY_TURNS,
  LARGE_SIZES,
  ProviderSpawnError,
  TabEvents,
  TEST_TIMEOUT_MS,
  TURN_TIMEOUT_MS,
  assertFinalStatuses,
  auditTranscript,
  captureProviderEvidence,
  completeCase,
  createWorkspace,
  disposeAgent,
  sendBusyMarkerWithRetry,
  makeLargePayload,
  openAgent,
  readChat,
  recordMetrics,
  repeatCount,
  submitBatch,
  submitLarge,
  submitTextWithoutAudit,
  toolOutputPrompt,
  uniqueMarker,
  verifyLargeReceipt,
  waitDelivered,
  waitForAndAuditRecords,
  waitForIdle,
  waitForWorking,
  withSpawnMetrics,
} from "./chat-send-matrix.live.helpers";
import type {
  AgentUi,
  ChatSendStatus,
  ProviderMetrics,
  SendRecord,
} from "./chat-send-matrix.live.helpers";

function requestedProviderIds(): Set<string> | undefined {
  const raw = process.env.PW_SEND_PROVIDERS;
  if (raw === undefined || raw.trim() === "") return undefined;
  const ids = raw.split(",").map((id) => id.trim()).filter(Boolean);
  expect(ids.length, "PW_SEND_PROVIDERS lists capability ids").toBeGreaterThan(0);
  return new Set(ids);
}

function hasProviderFailures(metrics: Map<string, ProviderMetrics>): boolean {
  return [...metrics.values()].some(
    (entry) =>
      !entry.skipped &&
      (Boolean(entry.errors?.length) ||
        Boolean(entry.spawnErrors?.length) ||
        entry.records.some(
          (record) =>
            record.receivedInSession !== true ||
            record.confirmedStatus !== true ||
            (record.markerCount ?? 0) > 1,
        )),
  );
}

test.describe.configure({ retries: 0 });

test("@live Chat UI sends are exact-once across available PTY providers", async ({
  api,
  ui,
  spawnAgent,
}, testInfo) => {
  test.setTimeout(TEST_TIMEOUT_MS);
  const repeats = repeatCount();
  const requestedIds = requestedProviderIds();
  const runId = randomUUID().replace(/-/g, "");
  const workspaceName = `pw-send-${runId}`;
  const capabilities = (await api.providers()).filter((provider) => provider.transport === "pty");
  if (requestedIds) {
    const capabilityIds = new Set(capabilities.map((provider) => provider.id));
    const unknownIds = [...requestedIds].filter((id) => !capabilityIds.has(id));
    expect(unknownIds, "PW_SEND_PROVIDERS matches advertised PTY capability ids").toEqual([]);
  }
  const providers = requestedIds
    ? capabilities.filter((provider) => requestedIds.has(provider.id))
    : capabilities;
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
  const spawnByProvider = new Map<string, SpawnAgent>();
  for (const provider of runnable) {
    spawnByProvider.set(
      provider.id,
      withSpawnMetrics(spawnAgent, metrics.get(provider.id)!, ownedTabs),
    );
  }
  let workspaceId: string | undefined;

  try {
    workspaceId = await createWorkspace(api, workspaceName);

    for (const [providerIndex, provider] of runnable.entries()) {
      const entry = metrics.get(provider.id)!;
      const providerOwnedTabStart = ownedTabs.length;
      const spawnProviderAgent = spawnByProvider.get(provider.id)!;

      try {
      const fresh = await openAgent(
        api,
        ui,
        spawnProviderAgent,
        provider,
        workspaceId,
        `pw-send-${providerIndex}-fresh`,
      );
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
        spawnProviderAgent,
        provider,
        workspaceId,
        `pw-send-${providerIndex}-busy`,
      );
      openStreams.add(busy.events);
      const busyPair = await sendBusyMarkerWithRetry(
        api,
        busy,
        entry,
        runId,
        "BUSY",
        "BUSY",
        0,
      );
      const busyRecords = [...busyPair.setupRecords, busyPair.followup];
      if (repeats > 1) {
        busyRecords.push(...(await submitBatch(
          busy,
          provider,
          entry,
          Array.from({ length: repeats - 1 }, (_, index) => {
            const marker = uniqueMarker(runId, provider, "BUSY", index + 1);
            return { text: `Queue this marker behind current work and preserve order: ${marker}`, marker };
          }),
          true,
        )));
      }
      await Promise.all(busyRecords.map((record) => waitDelivered(busy, record)));
      await assertFinalStatuses(api, busy, busyRecords);
      await auditTranscript(api, busy, busyRecords, entry);
      await waitForIdle(busy, api, "busy queued sends");
      await disposeAgent(api, busy);
      openStreams.delete(busy.events);

      const rapid = await openAgent(
        api,
        ui,
        spawnProviderAgent,
        provider,
        workspaceId,
        `pw-send-${providerIndex}-rapid`,
      );
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
        spawnProviderAgent,
        provider,
        workspaceId,
        `pw-send-${providerIndex}-large`,
      );
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
        spawnProviderAgent,
        provider,
        workspaceId,
        `pw-send-${providerIndex}-heavy`,
        "Orchestrator",
      );
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

      const heavyBusyPair = await sendBusyMarkerWithRetry(
        api,
        heavy,
        entry,
        runId,
        "HEAVY_BUSY",
        "HEAVY_BUSY",
        0,
      );
      const heavyBusyRecords = [...heavyBusyPair.setupRecords, heavyBusyPair.followup];
      if (repeats > 1) {
        heavyBusyRecords.push(...(await submitBatch(
          heavy,
          provider,
          entry,
          Array.from({ length: repeats - 1 }, (_, index) => {
            const marker = uniqueMarker(runId, provider, "HEAVY_BUSY", index + 1);
            return { text: `Busy heavy-context send ${index + 1}: ${marker}`, marker };
          }),
          true,
        )));
      }
      const busyPayload = makeLargePayload(LARGE_SIZES[1], runId, provider, 0, "HEAVY_BUSY_LARGE");
      const busyLarge = await submitLarge(api, heavy, provider, entry, ui, busyPayload);
      await Promise.all(heavyBusyRecords.map((record) => waitDelivered(heavy, record)));
      await assertFinalStatuses(api, heavy, [...heavyBusyRecords, busyLarge]);
      await auditTranscript(api, heavy, [...heavyBusyRecords, busyLarge], entry);
      await waitForIdle(heavy, api, "heavy-context busy queue");
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
          api, activate: heavy.activate,
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
      } catch (error) {
        if (!(error instanceof ProviderSpawnError)) {
          entry.errors ??= [];
          entry.errors.push(error instanceof Error ? error.message : String(error));
        }
        await captureProviderEvidence(api, entry);
        for (const events of [...openStreams]) {
          await events.close().catch(() => {});
          openStreams.delete(events);
        }
        const providerOwnedTabs = ownedTabs.splice(providerOwnedTabStart).reverse();
        for (const tabId of providerOwnedTabs) await api.closeTab(tabId).catch(() => {});
        console.log(
          `[chat-send-matrix-provider-error] ${JSON.stringify({
            provider: provider.display_name ?? provider.id,
            providerId: provider.id,
            error: error instanceof Error ? error.message : String(error),
          })}`,
        );
      }
    }

    if (!hasProviderFailures(metrics)) {
    const soakProvider = runnable[0];
    const soakMetrics = metrics.get(soakProvider.id)!;
    const soak = await openAgent(
      api,
      ui,
      spawnByProvider.get(soakProvider.id)!,
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
          spawnByProvider.get(provider.id)!,
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
    }

    const summaries = providers.map((provider) => recordMetrics(metrics.get(provider.id)!));
    console.log(`[chat-send-matrix-summary] ${JSON.stringify(summaries)}`);
    await testInfo.attach("chat-send-matrix-summary.json", {
      body: Buffer.from(JSON.stringify(summaries, null, 2)),
      contentType: "application/json",
    });
    const failures = summaries.flatMap((raw) => {
      const summary = raw as {
        provider: string;
        skipped?: string;
        sent: number;
        received_in_session: number;
        confirmed_status: number;
        duplicated: number;
        lost: number;
        unconfirmed: number;
        errors?: string[];
        spawn_errors?: string[];
      };
      if (summary.skipped) return [];
      const reasons: string[] = [];
      if (summary.lost > 0) reasons.push(`${summary.lost} lost`);
      if (summary.duplicated > 0) reasons.push(`${summary.duplicated} duplicated`);
      if (summary.unconfirmed > 0) reasons.push(`${summary.unconfirmed} unconfirmed`);
      if (summary.received_in_session !== summary.sent) {
        reasons.push(`${summary.received_in_session}/${summary.sent} received in session`);
      }
      if (summary.confirmed_status !== summary.sent) {
        reasons.push(`${summary.confirmed_status}/${summary.sent} confirmed status`);
      }
      for (const error of summary.spawn_errors ?? []) reasons.push(`spawn error: ${error}`);
      for (const error of summary.errors ?? []) reasons.push(error);
      return reasons.map((reason) => `${summary.provider}: ${reason}`);
    });
    expect(failures, "all selected providers complete with exact-once receipt and confirmed status").toEqual([]);
  } finally {
    for (const tabId of ownedTabs.reverse()) await api.closeTab(tabId).catch(() => {});
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
