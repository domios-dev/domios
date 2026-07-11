/**
 * Shared gate for specs that require the stub agent transport: the dev daemon
 * must run with CODETERM_STUB_AGENT_CMD (dev builds only). Probes once per
 * worker by spawning a claude pane and looking for the replayer banner.
 */
import type { Page } from "@playwright/test";
import { pollUntil } from "./poll";
import type { ApiClient } from "./api-client";

export async function stubPaneOutput(api: ApiClient, paneId: string): Promise<string> {
  // 404 while the PTY materializes after spawn is a normal warmup state.
  const res = await api
    .get<{ output?: string } | string>(`/panes/${paneId}/output`)
    .catch(() => "");
  return typeof res === "string" ? res : (res.output ?? "");
}

let stubActive: boolean | null = null;

export async function isStubTransportActive(api: ApiClient): Promise<boolean> {
  if (stubActive !== null) return stubActive;
  const spawned = await api
    .post<{ pane_id: string }>("/agents/spawn", { agent: "claude", title: "pw-stub-gate-probe" })
    .catch(() => null);
  if (!spawned) {
    stubActive = false;
    return stubActive;
  }
  const seen = await pollUntil(
    async () => (await stubPaneOutput(api, spawned.pane_id)).includes("stub replay"),
    30_000,
    500,
    "stub gate probe",
  ).catch(() => false);
  stubActive = Boolean(seen);
  await api.closePane(spawned.pane_id).catch(() => {});
  return stubActive;
}

/** Isolated workspace for stub agent flows: spawning Workers into the shared
 * General workspace can trip the canonical-checkout spawn guard when stray
 * state makes its cwd look like a primary git checkout. */
export async function createStubWorkspace(api: ApiClient): Promise<{ id: string; name: string }> {
  const name = `pw-stub-ws-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find((w) => w.name === name);
  if (!ws) throw new Error(`workspace ${name} missing after create`);
  // DOM oracles need the workspace mounted in the UI, not just existing.
  await api.focusWorkspace(ws.id);
  return { id: ws.id, name: ws.name };
}

/** Agent panes default to Chat view; terminal-DOM oracles need the Terminal
 * tab of the pane mode switch selected before polling xterm content. */
export async function showTerminalView(page: Page, paneId: string): Promise<void> {
  const switcher = page
    .locator(`[data-pane-id="${paneId}"] [data-testid="pane-mode-switch"]`)
    .first();
  await switcher.waitFor({ state: "visible", timeout: 30_000 });
  const terminalTab = switcher.locator('[role="tab"]', { hasText: "Terminal" }).first();
  if ((await terminalTab.getAttribute("aria-selected")) !== "true") {
    await terminalTab.click();
  }
}
