/**
 * Shared gate for specs that require the stub agent transport. The dev daemon
 * declares per provider whether it substitutes the replayer (`stub_replay`, debug
 * builds only); a local replayer also proves itself with `replay.mjs --health`.
 */
import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import type { Page } from "@playwright/test";
import type { ApiClient } from "./api-client";
import { normalizeStubAgentCmd, repoRoot } from "./dev-instance";

export async function stubTabOutput(api: ApiClient, tabId: string): Promise<string> {
  // 404 while the PTY materializes after spawn is a normal warmup state.
  const res = await api
    .get<{ output?: string } | string>(`/tabs/${tabId}/output`)
    .catch(() => "");
  return typeof res === "string" ? res : (res.output ?? "");
}

export const STUB_GATE_PROVIDER = "claude";

export type StubHealthReceipt = { ok: boolean; providers?: string[]; reason?: string };

export type StubGateDeps = {
  stubCommand: string | undefined;
  root: string;
  runHealth: (replayPath: string) => Promise<StubHealthReceipt>;
};

type StubProviderListing = { id: string; stub_replay?: boolean };

const REPLAY_PATH = /(?:^|\s)('[^']*replay\.mjs'|"[^"]*replay\.mjs"|[^\s'"]\S*replay\.mjs)(?=\s|$)/;

/** The replayer script the runner's stub command names, when it exists on this machine. */
export function localReplayPath(stubCommand: string | undefined, root: string): string | null {
  const token = normalizeStubAgentCmd(stubCommand, root)?.match(REPLAY_PATH)?.[1];
  if (!token) return null;
  const replayPath = token.replace(/^(['"])(.*)\1$/, "$2");
  return existsSync(replayPath) ? replayPath : null;
}

export function parseHealthReceipt(stdout: string, failure: string): StubHealthReceipt {
  const line = stdout.trim().split("\n").pop() ?? "";
  try {
    const receipt = JSON.parse(line) as StubHealthReceipt;
    if (typeof receipt.ok === "boolean") return receipt;
  } catch {
    /* fall through to the process failure */
  }
  return { ok: false, reason: failure || `unparseable health receipt: ${line.slice(0, 200)}` };
}

export function runReplayHealth(replayPath: string): Promise<StubHealthReceipt> {
  return new Promise((resolve) => {
    execFile(process.execPath, [replayPath, "--health"], (error, stdout, stderr) => {
      resolve(parseHealthReceipt(String(stdout), error ? `${error.message} ${String(stderr)}`.trim() : ""));
    });
  });
}

export async function resolveStubTransport(
  api: Pick<ApiClient, "get">,
  deps: StubGateDeps,
): Promise<boolean> {
  const providers = await api.get<StubProviderListing[]>("/agents/providers");
  if (providers.find((p) => p.id === STUB_GATE_PROVIDER)?.stub_replay !== true) return false;
  const replayPath = localReplayPath(deps.stubCommand, deps.root);
  if (replayPath === null) return true;
  const receipt = await deps.runHealth(replayPath);
  if (!receipt.ok) {
    throw new Error(`stub transport health check failed for ${replayPath}: ${receipt.reason ?? "no reason given"}`);
  }
  return true;
}

let stubActive: boolean | null = null;

export async function isStubTransportActive(api: ApiClient): Promise<boolean> {
  if (stubActive !== null) return stubActive;
  stubActive = await resolveStubTransport(api, {
    stubCommand: process.env.CODETERM_STUB_AGENT_CMD,
    root: repoRoot(),
    runHealth: runReplayHealth,
  });
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

/** Agent tabs default to Chat view; terminal-DOM oracles need the Terminal
 * tab of the tab mode switch selected before polling xterm content. */
export async function showTerminalView(page: Page, tabId: string): Promise<void> {
  const switcher = page
    .locator(`[data-tab-id="${tabId}"] [data-testid="tab-mode-switch"]`)
    .first();
  await switcher.waitFor({ state: "visible", timeout: 30_000 });
  const terminalTab = switcher.locator('[role="tab"]', { hasText: "Terminal" }).first();
  if ((await terminalTab.getAttribute("aria-selected")) !== "true") {
    await terminalTab.click();
  }
}
