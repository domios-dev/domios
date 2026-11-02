import type { Release, Ticket } from "./model";

export interface FrontendReleaseSummary { releaseId: string; version: string; notes: string }

export function frontendReleaseSummary(release: Release | undefined, ticket: Ticket | undefined, attempted: readonly string[]): FrontendReleaseSummary | null {
  if (!release || !ticket || release.releaseId !== ticket.releaseId || attempted.includes(release.releaseId)) return null;
  return { releaseId: release.releaseId, version: release.version,
    notes: (release.releaseNotes ?? "").split(/\r?\n/).map(line => line.replace(/^\s*[#*>-]+\s*/, "").trim()).filter(Boolean).slice(0, 3).join(" · ").slice(0, 480) };
}

let summary: FrontendReleaseSummary | null = null;
const listeners = new Set<() => void>();
export function appliedFrontendRelease() { return summary; }
export function subscribeAppliedFrontendRelease(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
export function publishAppliedFrontendRelease(next: FrontendReleaseSummary | null) {
  if (!next || summary?.releaseId === next.releaseId) return;
  summary = next;
  for (const listener of listeners) listener();
}
