export const BUILD_DRIFT_RECORD_KEY = "codeterm-build-drift-reload";
export const WEB_BOOT_CHANGED_EVENT = "codeterm-web-boot-changed";
export const WEB_EVENTS_READY_EVENT = "codeterm-web-events-ready";

export interface BootChangedDetail {
  boot: string;
  build?: string;
  relayed: boolean;
}

export interface DriftRecord {
  origin: string;
  build: string;
  at: number;
}

export type DaemonIdentity =
  | { kind: "health"; build: string | null; apiCompatible: boolean }
  | { kind: "hint"; build: string }
  | { kind: "unreachable" };

export interface DriftInput {
  isWeb: boolean;
  releasePage: boolean;
  relayedTrigger: boolean;
  loadedBuild: string;
  origin: string;
  daemon: DaemonIdentity;
  visible: boolean;
  record: DriftRecord | null;
  now: number;
}

export type DriftDecision =
  | { action: "none" }
  | { action: "clear" }
  | { action: "defer" }
  | { action: "reload"; record: DriftRecord }
  | { action: "banner"; reason: "api" | "build" | "build-unreachable" };

const NONE: DriftDecision = { action: "none" };

function knownBuild(build: string | null | undefined): build is string {
  return typeof build === "string" && build !== "" && build !== "dev" && build !== "unknown";
}

const MIN_BUILD_PREFIX = 7;

/** The bundle and the daemon abbreviate the same commit hash to different lengths. */
export function sameBuild(a: string, b: string): boolean {
  const [short, long] = a.length <= b.length ? [a, b] : [b, a];
  return short === long || (short.length >= MIN_BUILD_PREFIX && long.startsWith(short));
}

export function decideBuildDrift(input: DriftInput): DriftDecision {
  const { daemon } = input;
  if (daemon.kind === "health" && !daemon.apiCompatible) return { action: "banner", reason: "api" };
  if (!input.isWeb || input.releasePage || input.relayedTrigger) return NONE;
  if (!knownBuild(input.loadedBuild)) return NONE;
  const record = input.record?.origin === input.origin ? input.record : null;
  if (daemon.kind === "unreachable") {
    if (!record) return NONE;
    return sameBuild(record.build, input.loadedBuild) ? { action: "clear" } : { action: "banner", reason: "build-unreachable" };
  }
  if (!knownBuild(daemon.build)) return NONE;
  if (sameBuild(daemon.build, input.loadedBuild)) return { action: "clear" };
  if (record && sameBuild(record.build, daemon.build)) return { action: "banner", reason: "build" };
  if (!input.visible) return { action: "defer" };
  return { action: "reload", record: { origin: input.origin, build: daemon.build, at: input.now } };
}

export function readDriftRecord(storage: Pick<Storage, "getItem">): DriftRecord | null {
  try {
    const parsed: unknown = JSON.parse(storage.getItem(BUILD_DRIFT_RECORD_KEY) ?? "null");
    if (!parsed || typeof parsed !== "object") return null;
    const { origin, build, at } = parsed as Partial<DriftRecord>;
    return typeof origin === "string" && typeof build === "string" && typeof at === "number" ? { origin, build, at } : null;
  } catch {
    return null;
  }
}

/** True only when the record reads back byte-equal; a reload without it could loop. */
export function persistDriftRecord(storage: Pick<Storage, "getItem" | "setItem">, record: DriftRecord): boolean {
  try {
    const value = JSON.stringify(record);
    storage.setItem(BUILD_DRIFT_RECORD_KEY, value);
    return storage.getItem(BUILD_DRIFT_RECORD_KEY) === value;
  } catch {
    return false;
  }
}

export function clearDriftRecord(storage: Pick<Storage, "removeItem">): void {
  try {
    storage.removeItem(BUILD_DRIFT_RECORD_KEY);
  } catch {
    /* storage unavailable: nothing was retained */
  }
}

export function isSignedReleasePage(storage: Pick<Storage, "getItem">, bootKey: string): boolean {
  try {
    const parsed: unknown = JSON.parse(storage.getItem(bootKey) ?? "null");
    const ticket = parsed && typeof parsed === "object" ? (parsed as { ticket?: { releaseId?: unknown } }).ticket : undefined;
    return typeof ticket?.releaseId === "string" && ticket.releaseId !== "";
  } catch {
    return false;
  }
}
