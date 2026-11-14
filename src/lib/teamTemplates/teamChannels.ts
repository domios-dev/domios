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
  /** Refreshed, but none of its teams runs on this Domios; `reason` says why in plain words. */
  | { kind: "unused"; reason: string }
  /** The official channel falls back to the bundled teams, so its failure is a calm fact, not an alarm. */
  | { kind: "bundled"; reason: string }
  | { kind: "failed"; reason: string };

type SkippedTeam = import("../../../gen/SkippedTeam").SkippedTeam;
type SkipReason = import("../../../gen/SkipReason").SkipReason;

const SKIP_TEXT: Record<SkipReason, string> = {
  missing_package: "Package folder is missing",
  invalid_package: "Not a valid team package",
  requires_newer_domios: "Needs a newer Domios",
  reserved_name: "Uses a name reserved for built-in teams",
  index_mismatch: "Name or version differs from the channel's list",
  invalid_path: "Package sits outside the channel",
  invalid_index_entry: "Listed with an invalid version",
  duplicate_name: "Listed twice in the channel",
  host_policy: "Asks for something this Domios does not allow",
  other: "Could not be loaded",
};

export interface SkippedView {
  name: string;
  /** Plain words from the host's typed code; the host's own detail stays in `detail`, shown only on demand. */
  text: string;
  detail: string;
}

export function skippedView(entry: SkippedTeam): SkippedView {
  const needs = entry.code === "requires_newer_domios" ? entry.requiredDomios?.trim() : null;
  return { name: entry.name, text: needs ? `Needs Domios ${needs}` : SKIP_TEXT[entry.code] ?? SKIP_TEXT.other, detail: entry.reason };
}

export interface TeamChannelView {
  name: string;
  label: string;
  url: string;
  official: boolean;
  /** Only community channels can be removed; the official one is always registered. */
  removable: boolean;
  autoUpdates: boolean;
  refresh: ChannelRefresh;
  skipped: readonly SkippedView[];
}

/** Why a refreshed channel offers nothing on this Domios, from its typed skip codes. */
function unusedReason(skipped: readonly SkippedTeam[]): string {
  if (skipped.length === 0) return "it lists no teams";
  if (skipped.every((entry) => entry.code === "requires_newer_domios")) {
    const needs = new Set(skipped.map((entry) => entry.requiredDomios?.trim()).filter(Boolean));
    return needs.size === 1 ? `its teams need Domios ${[...needs][0]}` : "its teams need a newer Domios";
  }
  return "none of its teams can be used here";
}

export function teamChannelView(row: TeamChannelRow): TeamChannelView {
  const official = row.official || row.name === OFFICIAL_TEAM_CHANNEL;
  const outcome = row.lastRefresh?.trim();
  const refresh: ChannelRefresh = !outcome ? { kind: "never" }
    : outcome === "ok" ? (row.usableTeams === 0 ? { kind: "unused", reason: unusedReason(row.skippedEntries ?? []) } : { kind: "ok" })
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
    skipped: (row.skippedEntries ?? []).map(skippedView),
  };
}

/** The row's one status line: how it updates, then what it gives this Domios now. */
export function channelStatusLine(view: TeamChannelView): string {
  const updates = view.autoUpdates ? "Updates on its own" : "Updates only when you refresh";
  switch (view.refresh.kind) {
    case "unused":
      return view.official
        ? `${updates} · ${view.refresh.reason}, so the teams bundled with Domios are in use`
        : `${updates} · ${view.refresh.reason}`;
    // Offline the official channel keeps the teams bundled with Domios, which is a calm fact rather than an error.
    case "bundled": return `${updates} · using the teams bundled with Domios`;
    case "ok": return view.autoUpdates ? `${updates} · up to date` : updates;
    default: return updates;
  }
}

const REFUSAL_FALLBACK = "Couldn't finish that. Try again.";

/**
 * Plain words for a refused channel action. The host sends `{ code, error }` with a plain message; anything
 * without a typed code (a transport failure, a raw host error) gets the fallback, never the raw dump.
 */
export function channelRefusalText(error: unknown, fallback = REFUSAL_FALLBACK): string {
  const body = typeof (error as { body?: unknown } | null)?.body === "string" ? (error as { body: string }).body
    : error instanceof Error ? error.message : String(error);
  const start = body.indexOf("{");
  if (start < 0) return fallback;
  try {
    const value = JSON.parse(body.slice(start)) as { code?: unknown; error?: unknown };
    return typeof value.code === "string" && value.code !== "failed" && typeof value.error === "string" && value.error.trim()
      ? value.error.trim() : fallback;
  } catch {
    return fallback;
  }
}
