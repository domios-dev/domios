import { useSyncExternalStore } from "react";
import type { TeamChannelRow } from "../transport/teamTemplates";

export const OFFICIAL_TEAM_CHANNEL = "domios-teams";

/** Settings is a main view under the session dialog, so the dialog closes before this opens the Team channels card. */
export function openTeamChannelsSettings(): void {
  window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:team-channels" } }));
}

interface TeamChannelState {
  rows: readonly TeamChannelRow[] | null;
  /** Bumped by every add, refresh and remove, so open galleries reread their catalog. */
  changes: number;
}

let state: TeamChannelState = { rows: null, changes: 0 };
const listeners = new Set<() => void>();

function emit(next: TeamChannelState) {
  state = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The latest list read from the daemon; `changed` marks it as the answer to an owner action. */
export function publishTeamChannels(rows: readonly TeamChannelRow[], { changed = false }: { changed?: boolean } = {}) {
  emit({ rows, changes: state.changes + (changed ? 1 : 0) });
}

export function useTeamChannels(): readonly TeamChannelRow[] | null {
  return useSyncExternalStore(subscribe, () => state.rows, () => state.rows);
}

export function useTeamChannelChanges(): number {
  return useSyncExternalStore(subscribe, () => state.changes, () => state.changes);
}

/** A community channel's display name for its templates' source; null until the list is known. */
export function useTeamChannelLabel(name: string | null | undefined): string | null {
  const rows = useTeamChannels();
  if (!name) return null;
  return rows?.find((row) => row.name === name)?.displayName?.trim() || null;
}

export type ChannelRefresh =
  | { kind: "never" }
  | { kind: "ok" }
  /** The official channel falls back to the bundled teams, so its failure is a calm fact, not an alarm. */
  | { kind: "bundled"; reason: string }
  | { kind: "failed"; reason: string };

export interface TeamChannelView {
  name: string;
  label: string;
  url: string;
  official: boolean;
  /** Only community channels can be removed; the official one is always registered. */
  removable: boolean;
  autoUpdates: boolean;
  refresh: ChannelRefresh;
  skipped: readonly { name: string; reason: string }[];
}

export function teamChannelView(row: TeamChannelRow): TeamChannelView {
  const official = row.official || row.name === OFFICIAL_TEAM_CHANNEL;
  const outcome = row.lastRefresh?.trim();
  const refresh: ChannelRefresh = !outcome ? { kind: "never" }
    : outcome === "ok" ? { kind: "ok" }
    : official ? { kind: "bundled", reason: outcome }
    : { kind: "failed", reason: outcome };
  return {
    name: row.name,
    label: row.displayName?.trim() || row.name,
    url: row.url,
    official,
    removable: !official,
    autoUpdates: official,
    refresh,
    skipped: row.skippedEntries ?? [],
  };
}
