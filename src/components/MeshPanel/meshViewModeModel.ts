import { MESH_MACHINE_GROUP_ORDER, type MeshMachineGroupId } from "./meshMachineListModel";

export type MeshInventoryViewMode = "canvas" | "list";

export const MESH_LIST_DEFAULT_THRESHOLD = 8;
export const MESH_SUGGESTION_RESET_THRESHOLD = 6;
export const MESH_VIEW_MODE_STORAGE_KEY = "domios.mesh.viewMode";
export const MESH_LIST_SUGGESTION_DISMISSED_KEY = "domios.mesh.listSuggestionDismissed";
export const MESH_COLLAPSED_GROUPS_STORAGE_KEY = "domios.mesh.collapsedGroups";

const VALID_GROUP_IDS: ReadonlySet<string> = new Set(MESH_MACHINE_GROUP_ORDER);

export type MeshCollapsedGroupOverrides = Partial<Record<MeshMachineGroupId, boolean>>;

function defaultStorage(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

export function parseMeshViewMode(raw: string | null | undefined): MeshInventoryViewMode | null {
  return raw === "canvas" || raw === "list" ? raw : null;
}

export function readMeshViewMode(
  storage: Pick<Storage, "getItem"> | null = defaultStorage(),
): MeshInventoryViewMode | null {
  if (!storage) return null;
  try {
    return parseMeshViewMode(storage.getItem(MESH_VIEW_MODE_STORAGE_KEY));
  } catch {
    return null;
  }
}

export function writeMeshViewMode(
  mode: MeshInventoryViewMode,
  storage: Pick<Storage, "setItem"> | null = defaultStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(MESH_VIEW_MODE_STORAGE_KEY, mode);
  } catch {
    return;
  }
}

export function readListSuggestionDismissed(
  storage: Pick<Storage, "getItem"> | null = defaultStorage(),
): boolean {
  if (!storage) return false;
  try {
    return storage.getItem(MESH_LIST_SUGGESTION_DISMISSED_KEY) === "1";
  } catch {
    return false;
  }
}

export function writeListSuggestionDismissed(
  dismissed: boolean,
  storage: Pick<Storage, "setItem"> | null = defaultStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(MESH_LIST_SUGGESTION_DISMISSED_KEY, dismissed ? "1" : "0");
  } catch {
    return;
  }
}

export function readCollapsedMeshGroups(
  storage: Pick<Storage, "getItem"> | null = defaultStorage(),
): MeshCollapsedGroupOverrides {
  if (!storage) return {};
  try {
    const raw = storage.getItem(MESH_COLLAPSED_GROUPS_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const valid: MeshCollapsedGroupOverrides = {};
    for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (VALID_GROUP_IDS.has(key) && typeof value === "boolean") {
        valid[key as MeshMachineGroupId] = value;
      }
    }
    return valid;
  } catch {
    return {};
  }
}

export function writeCollapsedMeshGroups(
  overrides: MeshCollapsedGroupOverrides,
  storage: Pick<Storage, "setItem"> | null = defaultStorage(),
): void {
  if (!storage) return;
  try {
    const bounded: MeshCollapsedGroupOverrides = {};
    for (const id of MESH_MACHINE_GROUP_ORDER) {
      if (typeof overrides[id] === "boolean") bounded[id] = overrides[id];
    }
    storage.setItem(MESH_COLLAPSED_GROUPS_STORAGE_KEY, JSON.stringify(bounded));
  } catch {
    return;
  }
}

export function resolveInitialMeshViewMode(
  count: number,
  explicitChoice: MeshInventoryViewMode | null,
  narrow = false,
  threshold = MESH_LIST_DEFAULT_THRESHOLD,
): MeshInventoryViewMode {
  if (explicitChoice !== null) return explicitChoice;
  return narrow || count > threshold ? "list" : "canvas";
}

export interface MeshViewTransitionInput {
  previousCount: number | null;
  count: number;
  currentView: MeshInventoryViewMode;
  explicitChoice: MeshInventoryViewMode | null;
  dismissed: boolean;
  narrow: boolean;
  opening: boolean;
  suggestionVisible: boolean;
  threshold?: number;
  resetThreshold?: number;
}

export interface MeshViewTransitionResult {
  view: MeshInventoryViewMode;
  dismissed: boolean;
  suggestionVisible: boolean;
}

export function transitionMeshViewMode({
  previousCount,
  count,
  currentView,
  explicitChoice,
  dismissed,
  narrow,
  opening,
  suggestionVisible,
  threshold = MESH_LIST_DEFAULT_THRESHOLD,
  resetThreshold = MESH_SUGGESTION_RESET_THRESHOLD,
}: MeshViewTransitionInput): MeshViewTransitionResult {
  const nextDismissed = count <= resetThreshold ? false : dismissed;
  if (opening || previousCount === null) {
    return {
      view: resolveInitialMeshViewMode(count, explicitChoice, narrow, threshold),
      dismissed: nextDismissed,
      suggestionVisible: false,
    };
  }
  if (currentView === "list" || count <= threshold || nextDismissed) {
    return { view: currentView, dismissed: nextDismissed, suggestionVisible: false };
  }
  const crossedUp = previousCount <= threshold && count > threshold;
  return {
    view: currentView,
    dismissed: nextDismissed,
    suggestionVisible: crossedUp || suggestionVisible,
  };
}

export interface MeshViewAnswerResult {
  view: MeshInventoryViewMode;
  explicitChoice: MeshInventoryViewMode;
  dismissed: boolean;
  suggestionVisible: false;
}

export function answerSwitchToList(dismissed = false): MeshViewAnswerResult {
  return {
    view: "list",
    explicitChoice: "list",
    dismissed,
    suggestionVisible: false,
  };
}

export function answerKeepCanvas(): MeshViewAnswerResult {
  return {
    view: "canvas",
    explicitChoice: "canvas",
    dismissed: true,
    suggestionVisible: false,
  };
}

export function answerViewToggle(
  target: MeshInventoryViewMode,
  suggestionVisible: boolean,
  dismissed: boolean,
): MeshViewAnswerResult {
  if (target === "list") return answerSwitchToList(dismissed);
  return {
    view: "canvas",
    explicitChoice: "canvas",
    dismissed: dismissed || suggestionVisible,
    suggestionVisible: false,
  };
}
