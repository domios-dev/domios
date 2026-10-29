import type { UpdateAvailabilityItem } from "./updateAvailability";

export const UPDATE_INTENT_KEY = "codeterm.update-all.intent";

export type UpdateGroup = "App" | "Interface" | "Plugins";

export interface UpdateItem {
  id: string;
  group: UpdateGroup;
  name: string;
  currentVersion: string;
  targetVersion: string;
  channel?: string;
  channelError?: string;
  sourceWarning?: string;
}

export function updateSourceMessage(error: string): string {
  return error.match(/GitHub rate limit, retry at \d{2}:\d{2}/)?.[0] ?? error;
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

export function nextUpdateStep(intent: UpdateIntent, hostVersion: string, items: readonly UpdateItem[]): UpdateStep {
  if (intent.stage === "app" && hostVersion !== intent.appTarget) return { kind: "waitForApp" };
  const plugin = items.find(item => item.group === "Plugins" && intent.pluginIds.includes(item.id));
  if (plugin) return { kind: "plugin", id: plugin.id };
  if (intent.interfaceRequested && items.some(item => item.group === "Interface")) return { kind: "interface" };
  return { kind: "complete" };
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
  if (item?.state === "ready_to_apply") return item.kind === "frontend" && item.compatible && item.latest !== null;
  return item?.state === "update_available" && item.compatible
    && item.installed !== null && item.latest !== null && item.installed !== item.latest;
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
    .map(item => ({
    id: item.id,
    group: item.kind === "app" ? "App" : item.kind === "frontend" ? "Interface" : "Plugins",
    name: item.kind === "app" ? "Domios" : item.kind === "frontend" ? "Interface" : item.id,
    currentVersion: item.installed ?? "Bundled", targetVersion: item.latest ?? item.installed ?? "Unknown",
    channel: item.channel ?? undefined,
    channelError: item.kind === "plugin" && item.channel ? channelErrors[item.channel]
      : item.kind !== "plugin" && item.reason !== "cached_manifest" ? channelErrors[item.id] : undefined,
    sourceWarning: item.reason === "cached_manifest" ? channelErrors[item.id] : undefined,
  }));
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
export type UpdateRow = UpdateItem & { state: UpdateOutcome | "active" | "queued" | "available"; alreadyCurrent: boolean };

export interface UpdatePanelSummary {
  tone: "red" | "accent" | "green" | "muted";
  label: string;
  sentence: string;
}

function count(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

export function summarizeUpdatePanel(rows: readonly UpdateRow[], { busy, failed }: { busy: boolean; failed: boolean }): UpdatePanelSummary {
  const available = rows.filter(row => !row.channelError && row.state === "available").length;
  const completed = rows.filter(row => row.state === "completed").length;
  if (busy) {
    const run = rows.filter(row => row.state === "active" || row.state === "queued").length + completed;
    return { tone: "accent", label: "Updating components",
      sentence: run > 0 ? `Updating ${Math.min(completed + 1, run)} of ${run}` : "Updating components" };
  }
  if (failed) {
    const attempted = rows.filter(row => row.state === "failed" && !row.channelError).length;
    const unreachable = rows.some(row => row.channelError);
    return { tone: "red", label: "Update sources or actions failed",
      sentence: attempted ? `${count(attempted, "update")} failed` : unreachable ? "An update source is unavailable" : "Update failed" };
  }
  if (available) return { tone: "accent", label: "Updates available", sentence: `${count(available, "update")} available` };
  return { tone: completed ? "green" : "muted", label: "No applicable updates",
    sentence: completed ? "Everything is updated" : "Everything is up to date" };
}

export function targetInstalled(item: UpdateItem, snapshot: readonly UpdateAvailabilityItem[]): boolean {
  return snapshot.some(current => current.id === item.id && current.installed === item.targetVersion);
}

export function updateRunRows(runItems: readonly UpdateItem[], items: readonly UpdateItem[], snapshot: readonly UpdateAvailabilityItem[],
  intent: UpdateIntent | null, activeId: string | null, outcomes: Readonly<Record<string, UpdateOutcome>>): UpdateRow[] {
  return [...runItems.filter(item => item.group !== "Plugins" || snapshot.some(current => current.kind === "plugin" && current.id === item.id && current.installed !== null) || outcomes[item.id] !== undefined), ...items.filter(item => !runItems.some(row => row.id === item.id))].map(item => {
    const current = items.find(row => row.id === item.id) ?? item;
    const alreadyCurrent = targetInstalled(current, snapshot);
    const outcome = current.targetVersion === item.targetVersion ? outcomes[item.id] : undefined;
    const state = outcome === "failed" || current.channelError ? "failed" : alreadyCurrent ? "completed" : outcome ?? (activeId === item.id ? "active"
      : intent && (intent.pluginIds.includes(item.id) || (item.group === "Interface" && intent.interfaceRequested)) ? "queued" : "available");
    return { ...current, state, alreadyCurrent };
  });
}
