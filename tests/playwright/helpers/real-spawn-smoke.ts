// Real-launch contract: add every command/template type to this matrix and a
// @live smoke that calls `assertRealTemplateSpawn` against a real provider.
// @live pane UI assertions must activate the owning workspace+tab first; pane
// focus alone is intra-tab state and does not mount a hidden tab's pane.
// Without credentials, run only that title against an already-owned non-stub
// instance with `--timeout=30000 --global-timeout=600000 --trace=on`.
import { expect, type Page } from "@playwright/test";
import type { ApiClient } from "./api-client";
import { pollUntil } from "./poll";

export const REAL_SPAWN_TEMPLATE_TYPES = [
  "orchestration-pair",
  "single-agent",
] as const;

export type RealSpawnTemplateType = (typeof REAL_SPAWN_TEMPLATE_TYPES)[number];

export type RealSpawnSurface = {
  paneId: string;
  provider: string;
  label: string;
};

type PaneStatus = {
  agent_session_id?: string | null;
  agent_type?: string | null;
  status?: string | null;
};

type MuxState = {
  workspaces?: Array<{
    id: string;
    active_tab_id?: string | null;
    tabs?: Array<{
      id: string;
      pane_id: string;
    }>;
  }>;
};

export type MaterializedRealSpawn = RealSpawnSurface & {
  sessionId: string;
};

const PROCESS_TIMEOUT_MS = 180_000;
const FIRST_CHAT_TIMEOUT_MS = 120_000;
const STARTUP_STATE_TIMEOUT_MS = 30_000;
const PANE_MOUNT_TIMEOUT_MS = 5_000;
const CHAT_SETTLE_TIMEOUT_MS = 30_000;

function paneText(api: ApiClient, paneId: string): Promise<string> {
  return api
    .get<{ text?: string } | string>(`/panes/${paneId}/text`)
    .then((text) =>
      typeof text === "string" ? text : (text.text ?? JSON.stringify(text)),
    )
    .catch(() => "");
}

async function chatMessageCount(
  api: ApiClient,
  paneId: string,
): Promise<number> {
  const chat = await api
    .get<{ messages?: unknown[] }>(`/panes/${paneId}/chat`)
    .catch(() => ({ messages: [] }));
  return chat.messages?.length ?? 0;
}

async function paneTabRef(
  api: ApiClient,
  paneId: string,
): Promise<{ workspaceId: string; tabId: string }> {
  const mux = await api.get<MuxState>("/mux/state");
  for (const workspace of mux.workspaces ?? []) {
    const tab = workspace.tabs?.find((candidate) => candidate.pane_id === paneId);
    if (tab) return { workspaceId: workspace.id, tabId: tab.id };
  }
  throw new Error(`pane ${paneId} has no owning workspace tab in mux state`);
}

async function activeTabId(api: ApiClient, workspaceId: string): Promise<string | null> {
  const mux = await api.get<MuxState>("/mux/state");
  return mux.workspaces?.find((workspace) => workspace.id === workspaceId)?.active_tab_id ?? null;
}

function isBareShellPrompt(text: string): boolean {
  const clean = text.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "").replace(/\r/g, "");
  return /(?:^|\n)\s*(?:PS [^\n>]*>|[^\s\n]+@[^\s\n]+\s+(?:MINGW64|MSYS|CYGWIN)[^\n]*[$#]|(?:bash|zsh|sh)-[\d.]+[$#])\s*$/.test(
    clean,
  );
}

async function waitForProviderProcess(
  api: ApiClient,
  surface: RealSpawnSurface,
): Promise<MaterializedRealSpawn> {
  const materialized = await pollUntil(
    async () => {
      const pane = (await api.panes()).find(
        (candidate) => candidate.id === surface.paneId,
      );
      if (!pane?.pty_id || pane.agent_type !== surface.provider)
        return undefined;

      const status = await api
        .get<PaneStatus>(`/panes/${surface.paneId}/status`)
        .catch(() => ({}) as PaneStatus);
      const sessionId = status.agent_session_id;
      if (
        typeof sessionId !== "string" ||
        sessionId.length === 0 ||
        status.status === "Dead"
      ) {
        return undefined;
      }
      return { ...surface, sessionId };
    },
    PROCESS_TIMEOUT_MS,
    1_000,
    `${surface.label} real ${surface.provider} process and session materialize`,
  );
  if (!materialized)
    throw new Error(
      `${surface.label} did not materialize a real provider process`,
    );
  return materialized;
}

async function waitForFirstVisibleChatMessage(
  api: ApiClient,
  page: Page,
  surface: MaterializedRealSpawn,
): Promise<void> {
  const tabRef = await paneTabRef(api, surface.paneId);
  await api.post(`/panes/${surface.paneId}/chat-view`, { enabled: true });
  await api.focusWorkspace(tabRef.workspaceId);
  await api.post(`/workspaces/${tabRef.workspaceId}/tabs/${tabRef.tabId}/focus`);
  await api.focusPane(surface.paneId);

  const tab = page.locator(`[data-testid="tab-item"][data-tab-id="${tabRef.tabId}"]`).first();
  await expect(
    tab,
    `${surface.label} owning tab is selectable`,
  ).toBeVisible({
    timeout: PANE_MOUNT_TIMEOUT_MS,
  });
  await tab.click();
  await expect
    .poll(
      () => activeTabId(api, tabRef.workspaceId),
      {
        timeout: PANE_MOUNT_TIMEOUT_MS,
        intervals: [100, 250, 500],
        message: `${surface.label} owning tab becomes active before pane mount`,
      },
    )
    .toBe(tabRef.tabId);

  const chat = page.locator(
    `[data-testid="pane-session-chat"][data-pane-id="${surface.paneId}"]`,
  );
  const messages = chat.locator('[data-testid="chat-message"]');
  const settingUp = chat.locator('[data-testid="chat-setup-state"]');
  await expect(
    chat,
    `${surface.label} pane-session-chat mounts after owning tab activation`,
  ).toBeVisible({
    timeout: PANE_MOUNT_TIMEOUT_MS,
  });

  const initialSurface = await pollUntil(
    async () => {
      if (
        await messages
          .first()
          .isVisible()
          .catch(() => false)
      )
        return "first chat message";
      if (await settingUp.isVisible().catch(() => false))
        return "setting up state";
      return undefined;
    },
    CHAT_SETTLE_TIMEOUT_MS,
    250,
    `${surface.label} chat pipeline settles to first message or setup state`,
  );
  expect(initialSurface).toBeTruthy();

  await pollUntil(
    async () =>
      (await chatMessageCount(api, surface.paneId)) > 0 ? true : undefined,
    FIRST_CHAT_TIMEOUT_MS,
    1_000,
    `${surface.label} first chat message reaches the session tailer`,
  );
  await expect(
    messages.first(),
    `${surface.label} first chat message is visible`,
  ).toBeVisible({
    timeout: STARTUP_STATE_TIMEOUT_MS,
  });
}

async function assertNotBareShell(
  api: ApiClient,
  surface: MaterializedRealSpawn,
): Promise<void> {
  const text = await paneText(api, surface.paneId);
  expect(
    text,
    `${surface.label} must not use the replay transport`,
  ).not.toContain("stub replay");
  expect(
    isBareShellPrompt(text),
    `${surface.label} launched a bare shell instead of the ${surface.provider} provider`,
  ).toBe(false);
}

/** Shared real-launch oracle for status/session, UI first-message, and no-bare-shell checks. */
export async function assertRealTemplateSpawn(
  api: ApiClient,
  page: Page,
  template: RealSpawnTemplateType,
  surfaces: readonly RealSpawnSurface[],
): Promise<MaterializedRealSpawn[]> {
  expect(
    surfaces,
    `${template} must return at least one pane id`,
  ).not.toHaveLength(0);
  expect(
    new Set(surfaces.map((surface) => surface.paneId)).size,
    `${template} returned duplicate pane ids`,
  ).toBe(surfaces.length);

  const materialized = await Promise.all(
    surfaces.map((surface) => waitForProviderProcess(api, surface)),
  );
  for (const surface of materialized) {
    await waitForFirstVisibleChatMessage(api, page, surface);
  }
  await Promise.all(
    materialized.map((surface) => assertNotBareShell(api, surface)),
  );
  return materialized;
}
