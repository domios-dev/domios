import type { UpdateAvailabilityItem } from "./updateAvailability";

export const UPDATE_INTENT_KEY = "codeterm.update-all.intent";

export type UpdateGroup = "App" | "Interface" | "Plugins";

export interface UpdateItem {
  id: string;
  group: UpdateGroup;
  name: string;
  currentVersion: string;
  targetVersion: string;
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

export function panelItems(items: readonly UpdateAvailabilityItem[]): UpdateItem[] {
  return items.filter(hasUpdateBadge).map(item => ({
    id: item.id,
    group: item.kind === "app" ? "App" : item.kind === "frontend" ? "Interface" : "Plugins",
    name: item.kind === "app" ? "Domios" : item.kind === "frontend" ? "Interface" : item.id,
    currentVersion: item.installed ?? "Bundled", targetVersion: item.latest!,
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
