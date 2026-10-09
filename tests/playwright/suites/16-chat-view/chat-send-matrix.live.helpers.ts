import { createHash } from "node:crypto";
import { performance } from "node:perf_hooks";
import { expect, type ManagedUiSession, type SpawnAgent } from "../../helpers/fixtures";
import type { ApiClient, Provider } from "../../helpers/api-client";
export type ChatMessage = { id: string; type: string; content: string };
export type ChatSnapshot = { messages: ChatMessage[]; count: number; pending: boolean };
export type ChatSendState = "held" | "written" | "submit_unconfirmed" | "delivered" | "failed";
export type ChatSendStatus = {
  send_id: string;
  tab_id: string;
  state: ChatSendState;
  error: string | null;
  sequence: number;
};
export type ApiEvent = {
  seq: number;
  event_type: string;
  data: Record<string, unknown>;
  receivedAt: number;
};
export type SendRecord = {
  providerId: string;
  tabId: string;
  text: string;
  receiptText?: string;
  bubbleMarker: string;
  submittedAt: number;
  sequenceFloor?: number;
  sequence?: number;
  firstStatus?: ChatSendState;
  firstStatusAt?: number;
  deliveredAt?: number;
  sendId?: string;
  deliveryStatus?: ChatSendState | null;
  confirmedStatus?: boolean;
  markerCount?: number;
  receivedInSession?: boolean;
  sessionBound?: boolean;
  audited?: boolean;
  receiptCount?: number;
};
export type ProviderMetrics = {
  provider: Provider;
  records: SendRecord[];
  skipped?: string;
  errors?: string[];
  spawnErrors?: string[];
  spawnLatenciesMs?: Array<{ title: string; durationMs: number }>;
  busyAttempts?: Array<{
    attempt: number;
    setupSendId: string | null;
    outcome: "not_busy" | "sent_while_working";
    followupSendId?: string;
    statusWhenChecked?: string;
  }>;
};
export type AgentUi = {
  tabId: string;
  api: ApiClient;
  chat: import("@playwright/test").Locator;
  composer: import("@playwright/test").Locator;
  activate: () => Promise<void>;
  events: TabEvents;
  nextSendSequence: number;
};

export const SEND_TIMEOUT_MS = 120_000;
export const TURN_TIMEOUT_MS = 240_000;
export const TEST_TIMEOUT_MS = 7_200_000;
export const SPAWN_TIMEOUT_MS = 180_000;
export const SLOW_SPAWN_THRESHOLD_MS = 15_000;
export const BUSY_WORKING_TIMEOUT_MS = 60_000;
export const LARGE_SIZES = [8 * 1024, 40 * 1024] as const;
export const HEAVY_TURNS = 12;
export const HEAVY_OUTPUT_BYTES = 4 * 1024;

export class ProviderSpawnError extends Error {
  constructor(providerId: string, detail: string) {
    super(`${providerId} spawn failed: ${detail}`);
    this.name = "ProviderSpawnError";
  }
}

export function withSpawnMetrics(
  spawnAgent: SpawnAgent,
  metrics: ProviderMetrics,
  ownedTabs: string[],
): SpawnAgent {
  return async <T extends { tab_id: string } = { tab_id: string }>(body: Record<string, unknown>) => {
    const startedAt = performance.now();
    const title = typeof body.title === "string" ? body.title : "untitled";
    try {
      const spawned = await spawnAgent<T>(body, { timeoutMs: SPAWN_TIMEOUT_MS });
      ownedTabs.push(spawned.tab_id);
      return spawned;
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      metrics.spawnErrors ??= [];
      metrics.spawnErrors.push(detail);
      throw new ProviderSpawnError(metrics.provider.id, detail);
    } finally {
      metrics.spawnLatenciesMs ??= [];
      metrics.spawnLatenciesMs.push({ title, durationMs: performance.now() - startedAt });
    }
  };
}

export class TabEvents {
  private readonly controller = new AbortController();
  private readonly frames: ApiEvent[] = [];
  private readonly waiters: Array<{
    predicate: (event: ApiEvent) => boolean;
    resolve: (event: ApiEvent) => void;
    reject: (error: Error) => void;
    signal: AbortSignal;
    onAbort: () => void;
  }> = [];
  private failure: Error | undefined;
  private reader: ReadableStreamDefaultReader<Uint8Array> | undefined;

  private constructor() {}

  static async open(api: ApiClient, tabId: string): Promise<TabEvents> {
    const events = new TabEvents();
    const response = await api.openEventStream(tabId, events.controller.signal);
    if (!response.ok || !response.body) {
      throw new Error(`tab event stream failed for ${tabId}: HTTP ${response.status}`);
    }
    events.reader = response.body.getReader();
    void events.read();
    return events;
  }

  async close(): Promise<void> {
    this.controller.abort();
    await this.reader?.cancel().catch(() => {});
  }

  waitFor(
    predicate: (event: ApiEvent) => boolean,
    timeoutMs: number,
    label: string,
  ): Promise<ApiEvent> {
    const present = this.frames.find(predicate);
    if (present) return Promise.resolve(present);
    if (this.failure) return Promise.reject(this.failure);

    return new Promise((resolve, reject) => {
      const signal = AbortSignal.timeout(timeoutMs);
      let waiter!: (typeof this.waiters)[number];
      const finish = (error?: Error, event?: ApiEvent) => {
        signal.removeEventListener("abort", waiter.onAbort);
        const index = this.waiters.indexOf(waiter);
        if (index >= 0) this.waiters.splice(index, 1);
        if (error) reject(error);
        else resolve(event!);
      };
      waiter = {
        predicate,
        resolve,
        reject,
        signal,
        onAbort: () => finish(new Error(`${label} event timed out after ${timeoutMs}ms`)),
      };
      signal.addEventListener("abort", waiter.onAbort, { once: true });
      this.waiters.push(waiter);
      const arrived = this.frames.find(predicate);
      if (arrived) finish(undefined, arrived);
      else if (this.failure) finish(this.failure);
    });
  }

  latestSequence(): number {
    return this.frames.length === 0 ? 0 : this.frames[this.frames.length - 1].seq;
  }

  private async read(): Promise<void> {
    const reader = this.reader!;
    const decoder = new TextDecoder();
    let buffer = "";
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return;
        buffer += decoder.decode(value, { stream: true });
        const frames = buffer.split(/\r?\n\r?\n/);
        buffer = frames.pop() ?? "";
        for (const frame of frames) this.accept(frame);
      }
    } catch (error) {
      if (!this.controller.signal.aborted) {
        this.fail(error instanceof Error ? error : new Error(String(error)));
      }
    }
  }

  private accept(frame: string): void {
    const data = frame
      .split(/\r?\n/)
      .filter((line) => line.startsWith("data:"))
      .map((line) => line.slice(5).trimStart())
      .join("\n");
    if (!data) return;
    const parsed = JSON.parse(data) as Partial<ApiEvent>;
    if (parsed.event_type === "overflow") {
      this.fail(new Error(`tab event stream overflow: ${JSON.stringify(parsed.data)}`));
      return;
    }
    if (typeof parsed.seq !== "number" || typeof parsed.event_type !== "string") {
      this.fail(new Error(`malformed tab event frame: ${data.slice(0, 240)}`));
      return;
    }
    const event = {
      seq: parsed.seq,
      event_type: parsed.event_type,
      data: (parsed.data ?? {}) as Record<string, unknown>,
      receivedAt: performance.now(),
    };
    this.frames.push(event);
    for (const waiter of [...this.waiters]) {
      if (waiter.predicate(event)) {
        waiter.signal.removeEventListener("abort", waiter.onAbort);
        this.waiters.splice(this.waiters.indexOf(waiter), 1);
        waiter.resolve(event);
      }
    }
  }

  private fail(error: Error): void {
    this.failure = error;
    for (const waiter of [...this.waiters]) {
      waiter.signal.removeEventListener("abort", waiter.onAbort);
      waiter.reject(error);
    }
    this.waiters.splice(0);
  }
}

export function repeatCount(): number {
  const raw = process.env.PW_SEND_REPEAT;
  const value = raw === undefined ? 20 : Number(raw);
  if (!Number.isInteger(value) || value < 1) {
    throw new Error(`PW_SEND_REPEAT must be a positive integer, got ${raw}`);
  }
  return value;
}

export function statusEvent(event: ApiEvent, tabId: string): event is ApiEvent & { data: ChatSendStatus } {
  return event.event_type === "chat-send-status" && event.data.tab_id === tabId;
}

export function userMessages(snapshot: ChatSnapshot): ChatMessage[] {
  return snapshot.messages.filter((message) => message.type === "user");
}

export async function readChat(api: ApiClient, tabId: string): Promise<ChatSnapshot> {
  return api.get<ChatSnapshot>(`/tabs/${encodeURIComponent(tabId)}/chat?since=0`);
}

export async function createWorkspace(api: ApiClient, name: string): Promise<string> {
  await api.createWorkspace(name);
  const workspace = (await api.workspaces()).find((candidate) => candidate.name === name);
  if (!workspace) throw new Error(`workspace ${name} was not listed after creation`);
  return workspace.id;
}

export async function openAgent(
  api: ApiClient,
  ui: ManagedUiSession,
  spawnAgent: (body: Record<string, unknown>) => Promise<{ tab_id: string }>,
  provider: Provider,
  workspaceId: string,
  title: string,
  role?: "Orchestrator",
): Promise<AgentUi> {
  const launched = await spawnAgent({
    agent: provider.id,
    title,
    workspace_id: workspaceId,
    activate: true,
    can_spawn: false,
    ...(provider.supports_permission_bypass ? { skip_permissions: true } : {}),
    ...(role ? { role } : {}),
  });
  const tabId = launched.tab_id;
  expect(tabId, `${provider.display_name ?? provider.id} spawn returns its tab id`).toBeTruthy();
  await api.post(`/tabs/${encodeURIComponent(tabId)}/chat-view`, { enabled: true });
  const workspaceToggle = ui.page.getByTestId("workspaces-section-toggle");
  if (
    (await workspaceToggle.isVisible()) &&
    (await workspaceToggle.getAttribute("aria-expanded")) === "false"
  ) {
    await workspaceToggle.click();
  }
  const workspaceCard = ui.page.locator(
    `[data-testid="workspace-card"][data-workspace-id="${workspaceId}"]`,
  );
  const selectWorkspace = workspaceCard.getByRole("button", { name: /^Select workspace / });
  const row = ui.page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`);
  const activateTab = row.getByRole("button", { name: /^Open tab / });
  const chat = ui.page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"]`);
  const conversation = chat.getByRole("log", { name: "Conversation" });
  const composer = chat.getByRole("textbox", { name: "Message composer" });
  const activate = async () => {
    const showAllWorkspaces = ui.page.getByRole("button", { name: /^Show all workspaces,/ });
    if ((await workspaceCard.count()) === 0 && await showAllWorkspaces.isVisible()) {
      await showAllWorkspaces.click();
    }
    await expect(workspaceCard, `owned workspace ${workspaceId} is visible`).toBeVisible({
      timeout: 30_000,
    });
    await expect(selectWorkspace).toBeVisible({ timeout: 15_000 });
    if ((await selectWorkspace.getAttribute("aria-pressed")) !== "true") {
      await selectWorkspace.click();
      await expect(selectWorkspace).toHaveAttribute("aria-pressed", "true");
    }
    await api.focusWorkspace(workspaceId);

    await expect(row, `owned tab ${tabId} is visible in its workspace`).toBeVisible({
      timeout: 30_000,
    });
    await expect(activateTab).toBeVisible({ timeout: 15_000 });
    const tabIsActive = await row.locator(".active-tab-pill").isVisible();
    if (!tabIsActive) await activateTab.click();
    await api.post(
      `/workspaces/${encodeURIComponent(workspaceId)}/tabs/${encodeURIComponent(tabId)}/focus`,
    );
    await expect(row.locator(".active-tab-pill"), `owned tab ${tabId} is active`).toBeVisible({
      timeout: 30_000,
    });
    await expect(chat, `Chat UI mounted for ${tabId}`).toBeVisible({ timeout: 30_000 });
    await expect(chat).toHaveAttribute("data-tab-id", tabId);
    await expect(chat).toHaveAttribute("aria-busy", "false", { timeout: 30_000 });
    await expect(conversation).toBeVisible({ timeout: 15_000 });
    await expect(composer).toBeVisible({ timeout: 15_000 });
    await expect(composer, `composer for ${tabId} is enabled`).toBeEnabled({ timeout: 15_000 });

    const focusedWorkspace = (await api.workspaces()).find((workspace) => workspace.id === workspaceId);
    expect(focusedWorkspace?.is_active, `workspace ${workspaceId} is active before send`).toBe(true);
    const focusedTab = (await api.tabs()).find((tab) => tab.id === tabId);
    expect(focusedTab?.focused, `tab ${tabId} is focused before send`).toBe(true);
    await assertComposerBelongsToTab({ tabId, chat, composer });
  };
  await activate();

  const prior = await api.get<ChatSendStatus[]>(`/tabs/${encodeURIComponent(tabId)}/chat-send`);
  const nextSendSequence = Math.max(0, ...prior.map((status) => status.sequence));
  return {
    tabId,
    api,
    chat,
    composer,
    activate,
    events: await TabEvents.open(api, tabId),
    nextSendSequence,
  };
}

async function assertComposerBelongsToTab(agent: Pick<AgentUi, "tabId" | "chat" | "composer">): Promise<void> {
  const composerTabId = await agent.composer.evaluate((element) =>
    element.closest('[data-testid="tab-session-chat"]')?.getAttribute("data-tab-id"),
  );
  expect(composerTabId, `composer belongs to target tab ${agent.tabId}`).toBe(agent.tabId);
  await expect(agent.chat).toHaveAttribute("data-tab-id", agent.tabId);
  await expect(agent.composer, `composer for ${agent.tabId} is enabled before typing`).toBeEnabled();
}

export function makeRecord(
  providerId: string,
  agent: AgentUi,
  text: string,
  bubbleMarker: string,
  submittedAt: number,
): SendRecord {
  return {
    providerId,
    tabId: agent.tabId,
    text,
    receiptText: text,
    bubbleMarker,
    submittedAt,
    sequenceFloor: agent.nextSendSequence,
  };
}

export async function submitText(
  agent: AgentUi,
  provider: Provider,
  metrics: ProviderMetrics,
  text: string,
  bubbleMarker: string,
): Promise<SendRecord> {
  await agent.activate();
  await assertComposerBelongsToTab(agent);
  await agent.composer.fill(text);
  await expect(agent.composer).toHaveValue(text, { timeout: 15_000 });
  const submittedAt = performance.now();
  await agent.composer.press("Enter");
  const record = makeRecord(provider.id, agent, text, bubbleMarker, submittedAt);
  metrics.records.push(record);
  await expect(agent.composer, "Chat composer clears after submit").toHaveValue("", {
    timeout: 15_000,
  });

  const status = await agent.events.waitFor(
    (event) =>
      statusEvent(event, agent.tabId) &&
      typeof event.data.sequence === "number" &&
      event.data.sequence > agent.nextSendSequence,
    SEND_TIMEOUT_MS,
    `send admission ${bubbleMarker}`,
  );
  const data = status.data as ChatSendStatus;
  record.sendId = data.send_id;
  record.sequence = data.sequence;
  record.firstStatus = data.state;
  record.deliveryStatus = data.state;
  record.confirmedStatus = data.state === "delivered";
  record.firstStatusAt = status.receivedAt;
  agent.nextSendSequence = data.sequence;
  expect(data.state, `send ${bubbleMarker} was not refused`).not.toBe("failed");
  expect(data.error, `send ${bubbleMarker} did not report a delivery failure`).toBeNull();
  return record;
}

export async function submitPaste(
  agent: AgentUi,
  provider: Provider,
  metrics: ProviderMetrics,
  ui: ManagedUiSession,
  text: string,
  bubbleMarker: string,
): Promise<SendRecord> {
  await agent.activate();
  await assertComposerBelongsToTab(agent);
  await agent.composer.click();
  await ui.page.context().grantPermissions(["clipboard-read", "clipboard-write"], {
    origin: new URL(ui.page.url()).origin,
  });
  await ui.page.evaluate((value) => navigator.clipboard.writeText(value), text);
  await agent.composer.press(process.platform === "darwin" ? "Meta+V" : "Control+V");
  await expect(agent.composer, "clipboard paste reaches the real composer").toHaveValue(text, {
    timeout: 30_000,
  });
  const submittedAt = performance.now();
  await agent.composer.press("Enter");
  const record = makeRecord(provider.id, agent, text, bubbleMarker, submittedAt);
  metrics.records.push(record);
  await expect(agent.composer, "large Chat message clears after submit").toHaveValue("", {
    timeout: 30_000,
  });
  const status = await agent.events.waitFor(
    (event) =>
      statusEvent(event, agent.tabId) &&
      typeof event.data.sequence === "number" &&
      event.data.sequence > agent.nextSendSequence,
    SEND_TIMEOUT_MS,
    `large paste admission ${bubbleMarker}`,
  );
  const data = status.data as ChatSendStatus;
  record.sendId = data.send_id;
  record.sequence = data.sequence;
  record.firstStatus = data.state;
  record.deliveryStatus = data.state;
  record.confirmedStatus = data.state === "delivered";
  record.firstStatusAt = status.receivedAt;
  agent.nextSendSequence = data.sequence;
  expect(data.state, `large paste ${bubbleMarker} was not refused`).not.toBe("failed");
  expect(data.error, `large paste ${bubbleMarker} did not report a delivery failure`).toBeNull();
  return record;
}

export async function waitDelivered(agent: AgentUi, record: SendRecord): Promise<void> {
  expect(record.sequence, `send ${record.bubbleMarker} has an API ledger sequence`).toBeTruthy();
  if (record.firstStatus === "delivered") {
    record.deliveredAt = record.firstStatusAt;
    record.deliveryStatus = "delivered";
    record.confirmedStatus = true;
    return;
  }
  try {
    const delivered = await agent.events.waitFor(
      (event) =>
        statusEvent(event, agent.tabId) &&
        event.data.sequence === record.sequence &&
        event.data.state === "delivered",
      SEND_TIMEOUT_MS,
      `provider session receipt ${record.bubbleMarker}`,
    );
    record.deliveredAt = delivered.receivedAt;
    record.deliveryStatus = "delivered";
    record.confirmedStatus = true;
    return;
  } catch (eventError) {
    const statuses = await agent.api
      .get<ChatSendStatus[]>(`/tabs/${encodeURIComponent(agent.tabId)}/chat-send`)
      .catch(() => []);
    const status = statuses.find((candidate) => candidate.sequence === record.sequence);
    if (status?.state !== "delivered") throw eventError;
    record.sendId = status.send_id;
    record.deliveredAt = performance.now();
    record.deliveryStatus = "delivered";
    record.confirmedStatus = true;
  }
}

export async function submitTextWhileWorking(
  api: ApiClient,
  agent: AgentUi,
  provider: Provider,
  metrics: ProviderMetrics,
  text: string,
  bubbleMarker: string,
): Promise<SendRecord | undefined> {
  await agent.activate();
  await assertComposerBelongsToTab(agent);
  await agent.composer.fill(text);
  await expect(agent.composer).toHaveValue(text, { timeout: 15_000 });
  const tabStatus = await api.get<{ status: string }>(`/tabs/${encodeURIComponent(agent.tabId)}/status`);
  if (tabStatus.status !== "Working") {
    await agent.composer.fill("");
    return undefined;
  }

  const submittedAt = performance.now();
  await agent.composer.press("Enter");
  const record = makeRecord(provider.id, agent, text, bubbleMarker, submittedAt);
  metrics.records.push(record);
  await expect(agent.composer, "Chat composer clears after submit").toHaveValue("", {
    timeout: 15_000,
  });
  const status = await agent.events.waitFor(
    (event) =>
      statusEvent(event, agent.tabId) &&
      typeof event.data.sequence === "number" &&
      event.data.sequence > agent.nextSendSequence,
    SEND_TIMEOUT_MS,
    `busy send admission ${bubbleMarker}`,
  );
  const data = status.data as ChatSendStatus;
  record.sendId = data.send_id;
  record.sequence = data.sequence;
  record.firstStatus = data.state;
  record.deliveryStatus = data.state;
  record.confirmedStatus = data.state === "delivered";
  record.firstStatusAt = status.receivedAt;
  agent.nextSendSequence = data.sequence;
  expect(data.state, `busy send ${bubbleMarker} was not refused`).not.toBe("failed");
  expect(data.error, `busy send ${bubbleMarker} did not report a delivery failure`).toBeNull();
  return record;
}

export async function assertFinalStatuses(api: ApiClient, agent: AgentUi, records: SendRecord[]): Promise<void> {
  const statuses = await api.get<ChatSendStatus[]>(`/tabs/${encodeURIComponent(agent.tabId)}/chat-send`);
  for (const record of records) {
    const status = statuses.find((candidate) => candidate.sequence === record.sequence);
    if (status) {
      record.sendId = status.send_id;
      record.deliveryStatus = status.state;
      record.confirmedStatus = status.state === "delivered";
    }
    expect(status, `send ${record.bubbleMarker} remains in the delivery ledger`).toBeTruthy();
    expect(status!.state, `send ${record.bubbleMarker} reaches delivered`).toBe("delivered");
    expect(status!.error, `send ${record.bubbleMarker} has no submit_unconfirmed result`).toBeNull();
  }
}

export async function auditTranscript(
  api: ApiClient,
  agent: AgentUi,
  records: SendRecord[],
  metrics: ProviderMetrics,
): Promise<void> {
  const snapshot = await readChat(api, agent.tabId);
  const users = userMessages(snapshot);
  for (const record of records) {
    record.sessionBound = !snapshot.pending;
    record.markerCount = users.reduce(
      (count, message) => count + message.content.split(record.bubbleMarker).length - 1,
      0,
    );
    record.receivedInSession = record.sessionBound && record.markerCount === 1;
  }
  expect(snapshot.pending, `tab ${agent.tabId} transcript is bound to its own provider session`).toBe(false);
  let previousIndex = -1;
  for (const record of records) {
    expect(record.receiptText, `send ${record.bubbleMarker} has session-turn evidence`).toBeTruthy();
    const matches = users
      .map((message, index) => ({ message, index }))
      .filter(({ message }) => message.content === record.receiptText);
    record.receiptCount = matches.length;
    record.audited = true;
    expect(matches.length, `provider session contains ${record.bubbleMarker} exactly once`).toBe(1);
    if (matches[0]) {
      expect(matches[0].index, `provider user turns stay in send order for ${record.bubbleMarker}`).toBeGreaterThan(
        previousIndex,
      );
      previousIndex = matches[0].index;
    }
    const bubble = agent.chat
      .getByRole("article", { name: "user message" })
      .filter({ hasText: record.bubbleMarker });
    await expect(bubble, `Chat UI renders ${record.bubbleMarker} once`).toHaveCount(1, {
      timeout: 30_000,
    });
  }
}

export async function captureProviderEvidence(api: ApiClient, metrics: ProviderMetrics): Promise<void> {
  const tabIds = [...new Set(metrics.records.map((record) => record.tabId))];
  for (const tabId of tabIds) {
    const records = metrics.records.filter((record) => record.tabId === tabId);
    const unreceived = records.filter((record) => record.receivedInSession !== true);
    if (unreceived.length > 0) {
      try {
        const snapshot = await readChat(api, tabId);
        const users = userMessages(snapshot);
        for (const record of unreceived) {
          record.sessionBound = !snapshot.pending;
          record.markerCount = users.reduce(
            (count, message) => count + message.content.split(record.bubbleMarker).length - 1,
            0,
          );
          record.receivedInSession = record.sessionBound && record.markerCount === 1;
          if (record.receiptText !== undefined) {
            record.receiptCount = users.filter((message) => message.content === record.receiptText).length;
          }
        }
      } catch (error) {
        metrics.errors ??= [];
        metrics.errors.push(`session evidence for tab ${tabId}: ${error instanceof Error ? error.message : String(error)}`);
        for (const record of unreceived) record.receivedInSession ??= false;
      }
    }

    const unconfirmed = records.filter((record) => record.confirmedStatus !== true);
    if (unconfirmed.length > 0) {
      try {
        const statuses = await api.get<ChatSendStatus[]>(`/tabs/${encodeURIComponent(tabId)}/chat-send`);
        const unmatched = [...statuses];
        for (const record of unconfirmed) {
          const statusIndex = unmatched.findIndex(
            (candidate) =>
              (record.sequence !== undefined && candidate.sequence === record.sequence) ||
              (record.sendId !== undefined && candidate.send_id === record.sendId) ||
              (record.sequence === undefined &&
                candidate.sequence > (record.sequenceFloor ?? 0) &&
                candidate.sequence === Math.min(
                  ...unmatched
                    .filter((item) => item.sequence > (record.sequenceFloor ?? 0))
                    .map((item) => item.sequence),
                )),
          );
          const status = statusIndex >= 0 ? unmatched.splice(statusIndex, 1)[0] : undefined;
          if (!status) {
            record.confirmedStatus ??= false;
            continue;
          }
          record.sendId = status.send_id;
          record.sequence = status.sequence;
          record.deliveryStatus = status.state;
          record.confirmedStatus = status.state === "delivered";
        }
      } catch (error) {
        metrics.errors ??= [];
        metrics.errors.push(`delivery status for tab ${tabId}: ${error instanceof Error ? error.message : String(error)}`);
        for (const record of unconfirmed) record.confirmedStatus ??= false;
      }
    }
  }
}

export async function completeCase(
  api: ApiClient,
  agent: AgentUi,
  records: SendRecord[],
  metrics: ProviderMetrics,
): Promise<void> {
  await Promise.all(records.map((record) => waitDelivered(agent, record)));
  for (const record of records) {
    expect(record.deliveredAt, `send ${record.bubbleMarker} reached session evidence`).toBeTruthy();
  }
  await assertFinalStatuses(api, agent, records);
  await auditTranscript(api, agent, records, metrics);
}

export function agentState(agent: AgentUi) {
  return agent.chat.locator('[data-testid="agent-state"]');
}

export async function waitForWorkingEvent(
  agent: AgentUi,
  label: string,
  afterSequence: number,
): Promise<void> {
  await agent.events.waitFor(
    (event) =>
      event.seq > afterSequence &&
      event.event_type === "tab-status-changed" &&
      event.data.tab_id === agent.tabId &&
      event.data.status === "Working",
    BUSY_WORKING_TIMEOUT_MS,
    `${label} tab status Working`,
  );
}

export async function waitForWorking(agent: AgentUi, label: string): Promise<void> {
  await expect(agentState(agent), `${label} enters working state`).toHaveAttribute(
    "data-agent-state",
    "working",
    { timeout: TURN_TIMEOUT_MS },
  );
}

export async function waitForIdle(agent: AgentUi, api: ApiClient, label: string): Promise<void> {
  await expect(agentState(agent), `${label} returns to idle`).toHaveAttribute(
    "data-agent-state",
    "idle",
    { timeout: TURN_TIMEOUT_MS },
  );
  const status = await api.get<{ status: string; agent_session_id?: string | null }>(
    `/tabs/${encodeURIComponent(agent.tabId)}/status`,
  );
  expect(status.status, `${label} reaches backend idle`).toBe("Idle");
  expect(status.agent_session_id, `${label} has a bound provider session`).toBeTruthy();
}

export function uniqueMarker(runId: string, provider: Provider, scenario: string, index: number): string {
  const providerLabel = (provider.display_name ?? provider.id).replace(/[^A-Za-z0-9]/g, "_");
  return `PW_SEND_${runId}_${providerLabel}_${scenario}_${index}`;
}

export function makeLargePayload(
  bytes: number,
  runId: string,
  provider: Provider,
  index = 0,
  scenario = "LARGE",
): {
  text: string;
  start: string;
  end: string;
  sha256: string;
} {
  const start = uniqueMarker(runId, provider, `${scenario}_${bytes}_START`, index);
  const end = uniqueMarker(runId, provider, `${scenario}_${bytes}_END`, index);
  const head = `${start}\n\n\`\`\`ts\nconst fixture = [\n`;
  const line = `  "🧪 Привіт 世界 🌍",\n\n`;
  const tail = `  "${end}",\n];\n\`\`\`\n${end}`;
  const lines: string[] = [];
  while (Buffer.byteLength(head + [...lines, line].join("") + tail) + 3 <= bytes) {
    lines.push(line);
  }
  const base = head + lines.join("");
  const fillerBytes = bytes - Buffer.byteLength(base + tail);
  const filler = `//${"x".repeat(fillerBytes - 3)}\n`;
  const text = base + filler + tail;
  expect(Buffer.byteLength(text), "large paste generator hits its requested byte size").toBe(bytes);
  return {
    text,
    start,
    end,
    sha256: createHash("sha256").update(text, "utf8").digest("hex"),
  };
}

export function encodedNodeCommand(source: string): string {
  const encoded = Buffer.from(source, "utf8").toString("base64");
  return `node -e "eval(Buffer.from('${encoded}','base64').toString())"`;
}

export function toolOutputPrompt(marker: string, bytes: number): string {
  const source =
    `const text=${JSON.stringify(marker)}+"x".repeat(${bytes})+${JSON.stringify(`${marker}_END`)};` +
    `process.stdout.write(text);`;
  return `Run this exact command with your terminal tool, then stop: ${encodedNodeCommand(source)}`;
}

export function longAnswerPrompt(marker: string): string {
  return `Write exactly 400 short numbered list items, numbered 1 through 400, one per line. Do not use tools or stop early. Include this unique reference in your opening line: ${marker}`;
}

export async function sendBusyMarkerWithRetry(
  api: ApiClient,
  agent: AgentUi,
  metrics: ProviderMetrics,
  runId: string,
  setupScenario: string,
  followupScenario: string,
  followupIndex: number,
): Promise<{ setupRecords: SendRecord[]; followup: SendRecord }> {
  const provider = metrics.provider;
  const setupRecords: SendRecord[] = [];
  for (let attempt = 0; attempt < 2; attempt++) {
    const setupMarker = uniqueMarker(runId, provider, `${setupScenario}_WORK`, attempt);
    const statusSequenceFloor = agent.events.latestSequence();
    const setup = await submitTextWithoutAudit(
      agent,
      provider,
      metrics,
      longAnswerPrompt(setupMarker),
      setupMarker,
    );
    setupRecords.push(setup);

    try {
      await waitForWorkingEvent(agent, `${setupScenario} attempt ${attempt}`, statusSequenceFloor);
    } catch (error) {
      const status = await api.get<{ status: string }>(`/tabs/${encodeURIComponent(agent.tabId)}/status`);
      const timedOut = error instanceof Error && error.message.includes("event timed out");
      if (!timedOut) throw error;
      if (status.status !== "Idle") {
        throw new Error(`${setupScenario} did not reach Working; backend status is ${status.status}`);
      }
      await waitDelivered(agent, setup);
      metrics.busyAttempts ??= [];
      metrics.busyAttempts.push({
        attempt,
        setupSendId: setup.sendId ?? null,
        outcome: "not_busy",
        statusWhenChecked: status.status,
      });
      if (attempt === 0) continue;
      throw new Error(`${setupScenario} remained idle after one busy setup retry`);
    }

    const followupMarker = uniqueMarker(runId, provider, followupScenario, followupIndex);
    const followup = await submitTextWhileWorking(
      api,
      agent,
      provider,
      metrics,
      `Queue this marker while the long answer is working: ${followupMarker}`,
      followupMarker,
    );
    if (!followup) {
      const status = await api.get<{ status: string }>(`/tabs/${encodeURIComponent(agent.tabId)}/status`);
      if (status.status !== "Idle") {
        throw new Error(`${setupScenario} lost its Working event while backend status is ${status.status}`);
      }
      await waitDelivered(agent, setup);
      metrics.busyAttempts ??= [];
      metrics.busyAttempts.push({
        attempt,
        setupSendId: setup.sendId ?? null,
        outcome: "not_busy",
        statusWhenChecked: status.status,
      });
      if (attempt === 0) continue;
      throw new Error(`${setupScenario} finished before the second send after one setup retry`);
    }

    metrics.busyAttempts ??= [];
    metrics.busyAttempts.push({
      attempt,
      setupSendId: setup.sendId ?? null,
      outcome: "sent_while_working",
      followupSendId: followup.sendId,
      statusWhenChecked: "Working",
    });
    await Promise.all([waitDelivered(agent, setup), waitDelivered(agent, followup)]);
    return { setupRecords, followup };
  }
  throw new Error(`${setupScenario} busy setup exhausted`);
}

export function extractAbsolutePath(content: string): string | undefined {
  const found = content.match(/[A-Za-z]:[\\/][^\r\n]+|\/(?:[^\r\n]+)/)?.[0];
  return found?.replace(/["'`]+$/, "").trim();
}

export async function verifyLargeReceipt(
  api: ApiClient,
  agent: AgentUi,
  provider: Provider,
  metrics: ProviderMetrics,
  record: SendRecord,
  payload: ReturnType<typeof makeLargePayload>,
  runId: string,
): Promise<SendRecord | undefined> {
  const snapshot = await readChat(api, agent.tabId);
  const users = userMessages(snapshot);
  const matchedIndex = users.findIndex((message) => message.content === record.receiptText);
  expect(matchedIndex, `large paste ${record.bubbleMarker} exists in the provider session`).toBeGreaterThanOrEqual(0);
  const receivedText = users[matchedIndex].content;
  if (receivedText === payload.text) {
    expect(createHash("sha256").update(receivedText, "utf8").digest("hex")).toBe(payload.sha256);
    expect(receivedText.startsWith(payload.start)).toBe(true);
    expect(receivedText.endsWith(payload.end)).toBe(true);
    await auditTranscript(api, agent, [record], metrics);
    return undefined;
  }

  const path = extractAbsolutePath(receivedText);
  expect(path, `large send ${record.bubbleMarker} session turn points to its prepared body`).toBeTruthy();
  const proofToken = uniqueMarker(runId, provider, "HASH_PROOF", record.sequence ?? 0);
  const source =
    `const b=require("node:fs").readFileSync(${JSON.stringify(path)});` +
    `const c=require("node:crypto");` +
    `const v={byteLength:b.length,sha256:c.createHash("sha256").update(b).digest("hex"),` +
    `start:b.subarray(0,128).toString("utf8"),end:b.subarray(-128).toString("utf8")};` +
    `console.log(${JSON.stringify(proofToken)}+" "+JSON.stringify(v));`;
  const command = encodedNodeCommand(source);
  const probeText = `Read the exact prepared body at the path in this command and run it unchanged: ${command}`;
  const probe = await submitText(agent, provider, metrics, probeText, proofToken);
  await waitForWorking(agent, `large body hash ${record.bubbleMarker}`);
  await waitDelivered(agent, probe);
  await assertFinalStatuses(api, agent, [probe]);
  await auditTranscript(api, agent, [probe], metrics);
  await waitForIdle(agent, api, `large body hash ${record.bubbleMarker}`);

  const after = await readChat(api, agent.tabId);
  const proofRows = after.messages.filter(
    (message) => message.type === "tool_result" && message.content.includes(proofToken),
  );
  expect(proofRows.length, `provider tool output carries one body hash for ${record.bubbleMarker}`).toBe(1);
  const proofLine = proofRows[0].content
    .slice(proofRows[0].content.lastIndexOf(proofToken) + proofToken.length)
    .trim()
    .split(/\r?\n/, 1)[0]
    .trim();
  const proof = JSON.parse(proofLine) as {
    byteLength: number;
    sha256: string;
    start: string;
    end: string;
  };
  expect(proof.byteLength, `provider read every byte of ${record.bubbleMarker}`).toBe(
    Buffer.byteLength(payload.text),
  );
  expect(proof.sha256, `provider body hash matches ${record.bubbleMarker}`).toBe(payload.sha256);
  expect(proof.start, `provider read the start marker of ${record.bubbleMarker}`).toContain(payload.start);
  expect(proof.end, `provider read the end marker of ${record.bubbleMarker}`).toContain(payload.end);
  await auditTranscript(api, agent, [record], metrics);
}

export async function submitTextWithoutAudit(
  agent: AgentUi,
  provider: Provider,
  metrics: ProviderMetrics,
  text: string,
  marker: string,
): Promise<SendRecord> {
  return submitText(agent, provider, metrics, text, marker);
}

export async function submitBatch(
  agent: AgentUi,
  provider: Provider,
  metrics: ProviderMetrics,
  texts: Array<{ text: string; marker: string }>,
  requireWorking = false,
): Promise<SendRecord[]> {
  await agent.activate();
  const records: SendRecord[] = [];
  for (const item of texts) {
    const record = await submitTextWithoutAudit(agent, provider, metrics, item.text, item.marker);
    records.push(record);
    if (requireWorking) {
      await expect(agentState(agent), `busy send ${item.marker} is admitted while working`).toHaveAttribute(
        "data-agent-state",
        "working",
        { timeout: 15_000 },
      );
    }
  }
  return records;
}

export async function submitLarge(
  api: ApiClient,
  agent: AgentUi,
  provider: Provider,
  metrics: ProviderMetrics,
  ui: ManagedUiSession,
  payload: ReturnType<typeof makeLargePayload>,
): Promise<SendRecord> {
  const before = userMessages(await readChat(api, agent.tabId)).length;
  const record = await submitPaste(agent, provider, metrics, ui, payload.text, payload.start);
  await waitDelivered(agent, record);
  await assertFinalStatuses(api, agent, [record]);
  record.deliveredAt ??= record.submittedAt;
  const after = userMessages(await readChat(api, agent.tabId));
  expect(after.length, `large paste ${payload.start} adds one own-session user turn`).toBe(before + 1);
  record.receiptText = after[before]?.content;
  expect(record.receiptText, `large paste ${payload.start} has a provider-owned user turn`).toBeTruthy();
  const bubble = agent.chat
    .getByRole("article", { name: "user message" })
    .filter({ hasText: payload.start });
  await expect(bubble, `large paste ${payload.start} is shown once in Chat`).toHaveCount(1, {
    timeout: 30_000,
  });
  return record;
}

export async function waitForAndAuditRecords(
  api: ApiClient,
  agent: AgentUi,
  records: SendRecord[],
  metrics: ProviderMetrics,
): Promise<void> {
  await completeCase(api, agent, records, metrics);
}

export async function disposeAgent(api: ApiClient, agent: AgentUi): Promise<void> {
  await agent.events.close();
  await api.closeTab(agent.tabId).catch(() => {});
}

export function recordMetrics(metrics: ProviderMetrics): Record<string, unknown> {
  const receivedInSession = metrics.records.filter((record) => record.receivedInSession === true).length;
  const confirmedStatus = metrics.records.filter((record) => record.confirmedStatus === true).length;
  const duplicated = metrics.records.reduce(
    (total, record) => total + Math.max(0, (record.markerCount ?? 0) - 1),
    0,
  );
  const lost = metrics.records.filter(
    (record) => record.receivedInSession !== true,
  ).length;
  const unconfirmed = metrics.records.filter((record) => record.confirmedStatus !== true).length;
  const latencies = metrics.records
    .filter((record) => record.deliveredAt !== undefined)
    .map((record) => record.deliveredAt! - record.submittedAt);
  const spawnLatencies = metrics.spawnLatenciesMs ?? [];
  return {
    provider: metrics.provider.display_name ?? metrics.provider.id,
    providerId: metrics.provider.id,
    skipped: metrics.skipped,
    errors: metrics.errors,
    spawn_errors: metrics.spawnErrors,
    spawn_latencies_ms: spawnLatencies,
    slow_spawn_count: spawnLatencies.filter((latency) => latency.durationMs > SLOW_SPAWN_THRESHOLD_MS).length,
    max_spawn_latency_ms: Math.max(0, ...spawnLatencies.map((latency) => latency.durationMs)),
    busy_attempts: metrics.busyAttempts ?? [],
    sent: metrics.records.length,
    received_in_session: receivedInSession,
    confirmed_status: confirmedStatus,
    unconfirmed,
    duplicated,
    lost,
    sends: metrics.records.map((record) => ({
      send_id: record.sendId ?? null,
      tab_id: record.tabId,
      marker: record.bubbleMarker,
      received_in_session: record.receivedInSession ?? false,
      marker_count: record.markerCount ?? null,
      session_bound: record.sessionBound ?? false,
      confirmed_status: record.confirmedStatus ?? false,
      status: record.deliveryStatus ?? null,
      receipt_count: record.receiptCount ?? null,
    })),
    received: receivedInSession,
    maxComposerToFirstStatusMs: Math.max(
      0,
      ...metrics.records
        .filter((record) => record.firstStatusAt !== undefined)
        .map((record) => record.firstStatusAt! - record.submittedAt),
    ),
    maxComposerToDeliveredMs: Math.max(0, ...latencies),
  };
}
