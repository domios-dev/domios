import type { ChannelDiagnostic, UpdateFailure } from "./updateStatusModel";
import type { UpdateAvailabilityItem } from "./updateAvailability";

export const UPDATE_INTENT_KEY = "codeterm.update-all.intent";
export const CORE_PLUGIN_CHANNEL = "codeterm-core";

export type UpdateGroup = "App" | "Interface" | "Plugins";

export interface UpdateItem {
  id: string;
  group: UpdateGroup;
  name: string;
  currentVersion: string;
  /** A version strictly newer than the current one, or null when nothing newer is known. */
  targetVersion: string | null;
  channel?: string;
  channelError?: string;
  sourceWarning?: string;
  /** What the last successful check established, set while the row's source is unavailable. */
  lastKnown?: string;
}

export function updateSourceMessage(error: string): string {
  return error.match(/GitHub rate limit, retry at \d{2}:\d{2}/)?.[0]
    ?? error.replace(/^Failed to fetch release: /, "").replace(/^Plugin channel unavailable: /, "").replace(/ Domios will retry\.$/, "");
}

/** Names an availability error key: the app and interface feeds, discovery itself, or a plugin channel. */
export function updateSourceLabel(key: string): string {
  if (key === "app") return "App update channel";
  if (key === "frontend") return "Interface update channel";
  if (key === "discovery") return "Update discovery";
  return `${key} plugin channel`;
}

export function rowSourceKey(row: Pick<UpdateItem, "id" | "group" | "channel">): string {
  if (row.group === "App") return "app";
  if (row.group === "Interface") return "frontend";
  return row.channel ?? row.id;
}

export function unavailableUpdateSources(rows: readonly UpdateItem[], diagnostics: readonly ChannelDiagnostic[]): string[] {
  return [...new Set([
    ...rows.filter(row => row.channelError).map(row => updateSourceLabel(rowSourceKey(row))),
    ...diagnostics.filter(item => item.channel !== CORE_PLUGIN_CHANNEL).map(item => updateSourceLabel(item.channel)),
  ])];
}

export function unavailableSourcesSentence(sources: readonly string[]): string {
  return sources.length === 1 ? `${sources[0]} unavailable` : `${sources.length} update sources unavailable`;
}

export function groupUpdates(items: readonly UpdateItem[]) {
  return (["App", "Interface", "Plugins"] as const)
    .map(group => ({ group, items: items.filter(item => item.group === group) }))
    .filter(section => section.items.length > 0);
}

export interface UpdateIntent {
  appTarget: string | null;
  stage: "app" | "plugins" | "interface";
  pluginIds: string[];
  interfaceRequested: boolean;
}

export function createUpdateIntent(items: readonly UpdateItem[]): UpdateIntent {
  const app = items.find(item => item.group === "App");
  return {
    appTarget: app?.targetVersion ?? null,
    stage: app ? "app" : "plugins",
    pluginIds: items.filter(item => item.group === "Plugins").map(item => item.id),
    interfaceRequested: items.some(item => item.group === "Interface"),
  };
}

export type UpdateStep =
  | { kind: "waitForApp" }
  | { kind: "plugin"; id: string }
  | { kind: "interface" }
  | { kind: "complete" };

function parseAppVersion(value: string) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([\da-zA-Z-]+(?:\.[\da-zA-Z-]+)*))?(?:\+[\da-zA-Z-]+(?:\.[\da-zA-Z-]+)*)?$/.exec(value);
  if (!match) return null;
  const prerelease = match[4]?.split(".") ?? [];
  if (prerelease.some(part => /^0\d+$/.test(part))) return null;
  return { core: match.slice(1, 4).map(BigInt), prerelease };
}

function compareAppVersions(installed: string, target: string | null): number | null {
  const left = parseAppVersion(installed);
  const right = target === null ? null : parseAppVersion(target);
  if (!left || !right) return null;
  for (let i = 0; i < 3; i++) {
    if (left.core[i] !== right.core[i]) return left.core[i] > right.core[i] ? 1 : -1;
  }
  if (!left.prerelease.length || !right.prerelease.length) {
    return Number(!left.prerelease.length) - Number(!right.prerelease.length);
  }
  for (let i = 0; i < Math.max(left.prerelease.length, right.prerelease.length); i++) {
    const a = left.prerelease[i], b = right.prerelease[i];
    if (a === b) continue;
    if (a === undefined || b === undefined) return a === undefined ? -1 : 1;
    const numericA = /^\d+$/.test(a), numericB = /^\d+$/.test(b);
    if (numericA && numericB) return BigInt(a) > BigInt(b) ? 1 : -1;
    if (numericA !== numericB) return numericA ? -1 : 1;
    return a > b ? 1 : -1;
  }
  return 0;
}

export function appIntentSatisfied(intent: UpdateIntent, hostVersion: string): boolean {
  const comparison = compareAppVersions(hostVersion, intent.appTarget);
  return comparison !== null && comparison >= 0;
}

/** A restored app marker with nothing left to do once the running version reached its target. */
export function appIntentFinished(intent: UpdateIntent, hostVersion: string): boolean {
  return intent.stage === "app" && appIntentSatisfied(intent, hostVersion)
    && intent.pluginIds.length === 0 && !intent.interfaceRequested;
}

function isNewerVersion(installed: string, target: string): boolean {
  const comparison = compareAppVersions(installed, target);
  return comparison === null ? installed !== target : comparison < 0;
}

export function nextUpdateStep(intent: UpdateIntent, hostVersion: string, items: readonly UpdateItem[]): UpdateStep {
  if (intent.stage === "app" && !appIntentSatisfied(intent, hostVersion)) return { kind: "waitForApp" };
  const plugin = items.find(item => item.group === "Plugins" && intent.pluginIds.includes(item.id));
  if (plugin) return { kind: "plugin", id: plugin.id };
  if (intent.interfaceRequested && items.some(item => item.group === "Interface")) return { kind: "interface" };
  return { kind: "complete" };
}

export function restoredAppIntentInterrupted(intent: UpdateIntent, hostVersion: string,
  native: { observed: boolean; phase: string; installing: boolean }): boolean {
  return intent.stage === "app" && compareAppVersions(hostVersion, intent.appTarget) === -1
    && native.observed && native.phase === "idle" && !native.installing;
}

export function readUpdateIntent(value: string | null): UpdateIntent | null {
  if (!value) return null;
  try {
    const intent: unknown = JSON.parse(value);
    if (typeof intent !== "object" || intent === null) return null;
    const record = intent as Record<string, unknown>;
    if ((record.appTarget !== null && typeof record.appTarget !== "string")
      || !["app", "plugins", "interface"].includes(String(record.stage))
      || !Array.isArray(record.pluginIds) || !record.pluginIds.every(id => typeof id === "string")
      || typeof record.interfaceRequested !== "boolean") return null;
    return record as unknown as UpdateIntent;
  } catch { return null; }
}


export function hasUpdateBadge(item: UpdateAvailabilityItem | undefined): boolean {
  if (item?.state === "ready_to_apply") {
    return item.kind === "frontend" && item.compatible && item.latest !== null
      && (item.installed === null || isNewerVersion(item.installed, item.latest));
  }
  return item?.state === "update_available" && item.compatible
    && item.installed !== null && item.latest !== null && isNewerVersion(item.installed, item.latest);
}

function lastKnownUpdateState(item: UpdateAvailabilityItem): string {
  if (hasUpdateBadge(item)) return "Found at last check";
  if (item.latest === null || item.reason === "not_checked") return "Not checked yet";
  if (!item.compatible) return `${item.latest} is not compatible with this install`;
  return "Up to date at last check";
}

export function updateableItems(
  items: readonly UpdateAvailabilityItem[],
  channelErrors: Readonly<Record<string, string>>,
): UpdateAvailabilityItem[] {
  return items.filter(item => item.kind === "plugin" ? !item.channel || !channelErrors[item.channel]
    : !channelErrors[item.id] || (item.kind === "app" && item.reason === "cached_manifest"));
}

export function panelItems(
  items: readonly UpdateAvailabilityItem[],
  channelErrors: Readonly<Record<string, string>> = {},
): UpdateItem[] {
  return items.filter(item => hasUpdateBadge(item) || (item.kind !== "plugin" && Boolean(channelErrors[item.id])) || (item.kind === "plugin" && item.installed !== null && item.state !== "core_silent"
    && item.channel !== null && Boolean(channelErrors[item.channel])))
    .map(item => {
      const channelError = item.kind === "plugin" && item.channel ? channelErrors[item.channel]
        : item.kind !== "plugin" && item.reason !== "cached_manifest" ? channelErrors[item.id] : undefined;
      return {
        id: item.id,
        group: item.kind === "app" ? "App" : item.kind === "frontend" ? "Interface" : "Plugins",
        name: item.kind === "app" ? "Domios" : item.kind === "frontend" ? "Interface" : item.id,
        currentVersion: item.installed ?? "Bundled",
        targetVersion: hasUpdateBadge(item) ? item.latest : null,
        channel: item.channel ?? undefined,
        channelError,
        sourceWarning: item.reason === "cached_manifest" ? channelErrors[item.id] : undefined,
        lastKnown: channelError ? lastKnownUpdateState(item) : undefined,
      };
    });
}

export function availabilityIntent(items: readonly UpdateAvailabilityItem[]): UpdateIntent {
  const intent = createUpdateIntent(panelItems(items));
  if (intent.appTarget) {
    intent.pluginIds = items.filter(item => item.kind === "plugin" && item.installed !== null
      && (hasUpdateBadge(item) || item.state === "incompatible")).map(item => item.id);
  }
  return intent;
}


export type UpdateOutcome = "completed" | "failed";
export type UpdateRow = UpdateItem & { state: UpdateOutcome | "active" | "queued" | "available" | "unavailable"; alreadyCurrent: boolean };

export interface UpdatePanelSummary {
  tone: "red" | "warning" | "accent" | "green" | "muted";
  label: string;
  sentence: string;
}

function count(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

export function summarizeUpdatePanel(rows: readonly UpdateRow[], { busy, failed, unavailableSources = [] }: { busy: boolean; failed: boolean; unavailableSources?: readonly string[] }): UpdatePanelSummary {
  const available = rows.filter(row => !row.channelError && row.state === "available").length;
  const completed = rows.filter(row => row.state === "completed").length;
  if (busy) {
    const run = rows.filter(row => row.state === "active" || row.state === "queued").length + completed;
    return { tone: "accent", label: "Updating components",
      sentence: run > 0 ? `Updating ${Math.min(completed + 1, run)} of ${run}` : "Updating components" };
  }
  if (failed) {
    const attempted = rows.filter(row => row.state === "failed" && !row.channelError).length;
    return { tone: "red", label: "Update failed", sentence: attempted ? `${count(attempted, "update")} failed` : "Update failed" };
  }
  if (available) return { tone: "accent", label: "Updates available", sentence: `${count(available, "update")} available` };
  if (unavailableSources.length) return { tone: "warning", label: "Update source unavailable", sentence: unavailableSourcesSentence(unavailableSources) };
  return { tone: completed ? "green" : "muted", label: "No applicable updates",
    sentence: completed ? "Everything is updated" : "Everything is up to date" };
}

export function targetInstalled(item: UpdateItem, snapshot: readonly UpdateAvailabilityItem[]): boolean {
  return item.targetVersion !== null && snapshot.some(current => current.id === item.id && current.installed === item.targetVersion);
}

export function updateRunRows(runItems: readonly UpdateItem[], items: readonly UpdateItem[], snapshot: readonly UpdateAvailabilityItem[],
  intent: UpdateIntent | null, activeId: string | null, outcomes: Readonly<Record<string, UpdateOutcome>>): UpdateRow[] {
  return [...runItems.filter(item => item.group !== "Plugins" || snapshot.some(current => current.kind === "plugin" && current.id === item.id && current.installed !== null) || outcomes[item.id] !== undefined), ...items.filter(item => !runItems.some(row => row.id === item.id))].map(item => {
    const current = items.find(row => row.id === item.id) ?? item;
    const alreadyCurrent = targetInstalled(current, snapshot);
    const outcome = current.targetVersion === item.targetVersion ? outcomes[item.id] : undefined;
    const state = current.channelError ? "unavailable" : alreadyCurrent ? "completed" : outcome ?? (activeId === item.id ? "active"
      : intent && (intent.pluginIds.includes(item.id) || (item.group === "Interface" && intent.interfaceRequested)) ? "queued" : "available");
    return { ...current, state, alreadyCurrent };
  });
}

export function appendUpdateFailure(history: readonly UpdateFailure[], failure: UpdateFailure): UpdateFailure[] {
  return [...history.filter(previous => previous.message !== failure.message || previous.details !== failure.details), failure].slice(-10);
}
