/**
 * Reset helpers (testplan §4.4).
 *
 * Two tiers:
 * - `apiReset` — fast, between tests: closes every non-protected
 *   workspace/tab through the API, leaving the protected General
 *   workspace + its tabs as the known baseline.
 * - `wipeDevState` — heavy, suite-level: stops the daemon and deletes
 *   mutable state dirs under the named Playwright data directory,
 *   keeping config.toml (token!) and bin/. Caller must restart via
 *   `ensureDevConsole()` afterwards.
 *
 * HARD GUARD: every filesystem path this module touches must live under
 * a directory named `.codeterm-dev-*`. Prod (`~/.codeterm/`) and interactive
 * dev (`~/.codeterm-dev/`) are never touched — the harness throws instead.
 */
import { rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { ApiClient, devDataDirName } from "./api-client";
import { currentDevInstance } from "./dev-instance";
import { killDevConsole } from "./dev-console";

export function devDataDir(): string {
  return path.join(os.homedir(), devDataDirName(currentDevInstance()));
}

function assertDevPath(p: string): void {
  const normalized = p.replace(/\\/g, "/");
  if (!/\/\.codeterm-dev-[^/]+(?:\/|$)/.test(normalized)) {
    throw new Error(`prod-contamination guard: refusing to touch non-dev path: ${p}`);
  }
}

export async function apiReset(api: ApiClient): Promise<void> {
  for (const ws of await api.workspaces()) {
    if (!ws.protected) {
      await api.closeWorkspace(ws.id).catch(() => {});
    }
  }
  for (const tab of await api.tabs()) {
    if (!tab.protected) {
      await api.closeTab(tab.id).catch(() => {});
    }
  }
}

export type ResetBaseline = { workspaces: Set<string>; tabs: Set<string> };

export async function snapshotBaseline(api: ApiClient): Promise<ResetBaseline> {
  return {
    workspaces: new Set((await api.workspaces()).map((w) => w.id)),
    tabs: new Set((await api.tabs()).map((p) => p.id)),
  };
}

/** Close only what appeared after the baseline; pre-existing state is untouched. */
export async function diffReset(api: ApiClient, baseline: ResetBaseline): Promise<void> {
  for (const ws of await api.workspaces()) {
    if (!ws.protected && !baseline.workspaces.has(ws.id)) {
      await api.closeWorkspace(ws.id).catch(() => {});
    }
  }
  for (const tab of await api.tabs()) {
    if (!tab.protected && !baseline.tabs.has(tab.id)) {
      await api.closeTab(tab.id).catch(() => {});
    }
  }
}

/** Mutable state dirs that may be wiped without losing the token or CLI. */
const WIPEABLE = [
  "sessions",
  "workspaces",
  "session.json",
  "crons",
  "plugins",
  "channels",
  "plugin-channels.json",
  "plugin-audit.jsonl",
];

export function wipeDevState(): void {
  killDevConsole(currentDevInstance());
  for (const entry of WIPEABLE) {
    const target = path.join(devDataDir(), entry);
    assertDevPath(target);
    rmSync(target, { recursive: true, force: true });
  }
}
