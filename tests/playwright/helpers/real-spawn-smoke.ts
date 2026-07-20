// Real-launch contract: add every command/template type to this matrix and a
// @live smoke that calls `assertRealTemplateSpawn` against a real provider.
// @live pane UI assertions must activate the owning workspace+tab first; pane
// focus alone is intra-tab state and does not mount a hidden tab's pane.
// Without credentials, run only that title against an already-owned non-stub
// instance with `--timeout=30000 --global-timeout=600000 --trace=on`.
import { expect, type Locator, type Page } from "@playwright/test";
import type { ApiClient } from "./api-client";

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

const STARTUP_STATE_TIMEOUT_MS = 30_000;
const PANE_MOUNT_TIMEOUT_MS = 5_000;

function paneText(api: ApiClient, paneId: string): Promise<string> {
  return api
    .get<{ text?: string } | string>(`/panes/${paneId}/text`)
    .then((text) =>
      typeof text === "string" ? text : (text.text ?? JSON.stringify(text)),
    )
    .catch(() => "");
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

function isBareShellPrompt(text: string): boolean {
  const clean = text.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "").replace(/\r/g, "");
  return /(?:^|\n)\s*(?:PS [^\n>]*>|[^\s\n]+@[^\s\n]+\s+(?:MINGW64|MSYS|CYGWIN)[^\n]*[$#]|(?:bash|zsh|sh)-[\d.]+[$#])\s*$/.test(
    clean,
  );
}

async function assertProviderProcess(
  api: ApiClient,
  surface: RealSpawnSurface,
): Promise<MaterializedRealSpawn> {
  const pane = (await api.panes()).find(
    (candidate) => candidate.id === surface.paneId,
  );
  expect(pane?.pty_id, `${surface.label} owns a live PTY`).toBeTruthy();
  expect(pane?.agent_type, `${surface.label} uses the requested provider`).toBe(
    surface.provider,
  );

  const status = await api.get<PaneStatus>(`/panes/${surface.paneId}/status`);
  expect(status.status, `${surface.label} provider process stays live`).not.toBe(
    "Dead",
  );
  expect(
    status.agent_session_id,
    `${surface.label} owns a provider session after its first chat event`,
  ).toBeTruthy();
  return { ...surface, sessionId: status.agent_session_id as string };
}

export async function activateRealSpawnChat(
  api: ApiClient,
  page: Page,
  surface: RealSpawnSurface,
): Promise<Locator> {
  const tabRef = await paneTabRef(api, surface.paneId);
  await api.post(`/panes/${surface.paneId}/chat-view`, { enabled: true });
  await api.focusWorkspace(tabRef.workspaceId);
  await api.post(`/workspaces/${tabRef.workspaceId}/tabs/${tabRef.tabId}/focus`);
  await api.focusPane(surface.paneId);

  const tab = page
    .locator(`[data-testid="tab-item"][data-tab-id="${tabRef.tabId}"]`)
    .first()
    .getByRole("button", { name: /^Open tab / });
  await expect(
    tab,
    `${surface.label} owning tab is selectable`,
  ).toBeVisible({
    timeout: PANE_MOUNT_TIMEOUT_MS,
  });
  await tab.click();

  const chat = page.locator(
    `[data-testid="pane-session-chat"][data-pane-id="${surface.paneId}"]`,
  );
  const messages = chat.getByRole("article", { name: "assistant message" });
  await expect(
    chat,
    `${surface.label} pane-session-chat mounts after owning tab activation`,
  ).toBeVisible({
    timeout: PANE_MOUNT_TIMEOUT_MS,
  });
  await expect(
    messages.first(),
    `${surface.label} first assistant chat event is visible`,
  ).toBeVisible({
    timeout: STARTUP_STATE_TIMEOUT_MS,
  });
  return chat;
}

export async function expectRealSpawnIdle(
  api: ApiClient,
  page: Page,
  surface: RealSpawnSurface,
): Promise<void> {
  const tabRef = await paneTabRef(api, surface.paneId);
  const tab = page.locator(
    `[data-testid="tab-item"][data-tab-id="${tabRef.tabId}"]`,
  );
  const status = tab.getByRole("status");
  const initial = await api.get<PaneStatus>(
    `/panes/${surface.paneId}/status`,
  );
  const mustObserveBusy = initial.status !== "Idle";

  await status.evaluate((node, requireBusyTransition) => {
    const isIdle = () =>
      node.getAttribute("aria-label")?.endsWith(" tab status Idle") === true;
    let observedBusy = !isIdle();
    if (isIdle() && (!requireBusyTransition || observedBusy)) return;

    return new Promise<void>((resolve) => {
      const observer = new MutationObserver(() => {
        if (!isIdle()) observedBusy = true;
        if (isIdle() && (!requireBusyTransition || observedBusy)) {
          observer.disconnect();
          resolve();
        }
      });
      observer.observe(node, {
        attributes: true,
        attributeFilter: ["aria-label"],
      });
    });
  }, mustObserveBusy);

  const snapshot = await api.get<PaneStatus>(`/panes/${surface.paneId}/status`);
  expect(snapshot.status, `${surface.label} is Idle after the UI transition`).toBe(
    "Idle",
  );
}

export async function expectFreshAssistantReply(
  api: ApiClient,
  page: Page,
  surface: RealSpawnSurface,
  request: string,
  expected: string,
): Promise<void> {
  expect(request, "reply oracle must not self-match its request").not.toContain(
    expected,
  );
  const chat = await activateRealSpawnChat(api, page, surface);
  const assistantMessages = chat.getByRole("article", {
    name: "assistant message",
  });
  const baseline = await assistantMessages.count();

  await api.post(`/panes/${surface.paneId}/send`, {
    text: request,
    enter: true,
  });

  const freshAssistant = assistantMessages.nth(baseline);
  await expect(
    freshAssistant,
    `${surface.label} emits a fresh assistant event after the explicit probe`,
  ).toBeVisible({ timeout: STARTUP_STATE_TIMEOUT_MS });
  await expect(freshAssistant).toContainText(expected, {
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

  const materialized: MaterializedRealSpawn[] = [];
  for (const surface of surfaces) {
    await activateRealSpawnChat(api, page, surface);
    materialized.push(await assertProviderProcess(api, surface));
  }
  await Promise.all(
    materialized.map((surface) => assertNotBareShell(api, surface)),
  );
  return materialized;
}
