/**
 * Reset helpers (testplan §4.4).
 *
 * Two tiers:
 * - `apiReset` — fast, between tests: closes every non-protected
 *   workspace/pane through the API, leaving the protected General
 *   workspace + its panes as the known baseline.
 * - `wipeDevState` — heavy, suite-level: stops the daemon and deletes
 *   mutable state dirs under ~/.codeterm-dev/ (sessions, workspaces),
 *   keeping config.toml (token!) and bin/. Caller must restart via
 *   `ensureDevConsole()` afterwards.
 *
 * HARD GUARD: every filesystem path this module touches must live under
 * a directory literally named `.codeterm-dev`. Prod (`~/.codeterm/`) is
 * never touched — the harness throws rather than risk it (testplan Q9).
 */
import { rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { ApiClient } from "./api-client";
import { killDevConsole } from "./dev-console";

export function devDataDir(): string {
  return path.join(os.homedir(), ".codeterm-dev");
}

function assertDevPath(p: string): void {
  const normalized = p.replace(/\\/g, "/");
  if (!normalized.includes("/.codeterm-dev/") && !normalized.endsWith("/.codeterm-dev")) {
    throw new Error(`prod-contamination guard: refusing to touch non-dev path: ${p}`);
  }
}

export async function apiReset(api: ApiClient): Promise<void> {
  for (const ws of await api.workspaces()) {
    if (!ws.protected) {
      await api.closeWorkspace(ws.id).catch(() => {});
    }
  }
  for (const pane of await api.panes()) {
    if (!pane.protected) {
      await api.closePane(pane.id).catch(() => {});
    }
  }
}

/** Mutable state dirs that may be wiped without losing the token or CLI. */
const WIPEABLE = ["sessions", "workspaces", "session.json", "crons"];

export function wipeDevState(): void {
  killDevConsole();
  for (const entry of WIPEABLE) {
    const target = path.join(devDataDir(), entry);
    assertDevPath(target);
    rmSync(target, { recursive: true, force: true });
  }
}
