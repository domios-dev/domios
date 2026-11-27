export const SIDEBAR_MACHINE_GROUPS_KEY = "codeterm.sidebar.machine-groups-expanded";
export const MAX_MACHINE_GROUP_PREFS = 64;
export const MACHINE_COLLAPSE_THRESHOLD = 2;

export type MachineGroupPrefs = Record<string, boolean>;

export function machineGroupStorageId(peerId: string | null): string {
  return peerId || "local";
}

export function defaultMachineGroupExpanded(peerId: string | null, totalMachines: number): boolean {
  if (peerId === null) return true;
  return totalMachines <= MACHINE_COLLAPSE_THRESHOLD;
}

export function isMachineGroupExpanded(
  peerId: string | null,
  totalMachines: number,
  prefs: Readonly<MachineGroupPrefs>,
): boolean {
  const key = machineGroupStorageId(peerId);
  if (Object.prototype.hasOwnProperty.call(prefs, key)) {
    return prefs[key];
  }
  return defaultMachineGroupExpanded(peerId, totalMachines);
}

export function setMachineGroupExpanded(
  prefs: Readonly<MachineGroupPrefs>,
  peerId: string | null,
  expanded: boolean,
  maxEntries = MAX_MACHINE_GROUP_PREFS,
): MachineGroupPrefs {
  const key = machineGroupStorageId(peerId);
  const entries = Object.entries(prefs).filter(([entryKey]) => entryKey !== key);
  entries.push([key, expanded]);
  const limit = Math.max(1, maxEntries);
  const bounded = entries.length > limit ? entries.slice(entries.length - limit) : entries;
  return Object.fromEntries(bounded);
}

export function sanitizeMachineGroupPrefs(raw: unknown, maxEntries = MAX_MACHINE_GROUP_PREFS): MachineGroupPrefs {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return {};
  const valid = Object.entries(raw as Record<string, unknown>)
    .filter(([key, value]) => key.length > 0 && typeof value === "boolean") as Array<[string, boolean]>;
  const limit = Math.max(1, maxEntries);
  const bounded = valid.length > limit ? valid.slice(valid.length - limit) : valid;
  return Object.fromEntries(bounded);
}

export function loadMachineGroupPrefs(): MachineGroupPrefs {
  try {
    const raw = window.localStorage.getItem(SIDEBAR_MACHINE_GROUPS_KEY);
    if (!raw) return {};
    return sanitizeMachineGroupPrefs(JSON.parse(raw));
  } catch {
    return {};
  }
}

export function persistMachineGroupPrefs(prefs: Readonly<MachineGroupPrefs>): void {
  try {
    window.localStorage.setItem(SIDEBAR_MACHINE_GROUPS_KEY, JSON.stringify(sanitizeMachineGroupPrefs(prefs)));
  } catch {}
}
