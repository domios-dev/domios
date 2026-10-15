import type { Tab } from "./types";

export function shouldWakeOnOpen(
  tab: Partial<Tab> | null | undefined,
  isVisible: boolean,
  alreadyRequested: boolean,
): boolean {
  const missingBackend = tab?.backing?.kind === "pty" && !tab.pty_id && !tab.protected
    && !tab.close_restore?.stopped && tab.boot_stage !== "spawn_failed";
  return Boolean(tab && (tab.frozen || missingBackend) && isVisible && !alreadyRequested);
}
