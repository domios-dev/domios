import { carryCodexProjectTrust } from "./trust-carryover";

export interface ProfileSweepHost {
  platform(): string;
  homeDir(): string | null;
  log?(level: "warn", message: string): void;
  fs: {
    fileExists(path: string): boolean;
    isSymlink(path: string): boolean | null;
    readDir(path: string): { name: string; isFile: boolean }[];
    readFile(path: string): string | null;
    removeFile(path: string): boolean;
    writeFileAtomic(path: string, contents: string): boolean;
    lockFile(path: string): string | null;
    unlockFile(id: string): boolean;
  };
}

export interface ProfileSweepReport {
  dead: string[];
  legacy: string[];
  removed: string[];
}

const CODEX_PROFILE_NAME_RE = /^codeterm-([A-Za-z0-9_-]+?)(?:-(?:preserved(?:-\d+-\d+)?|retry-\d+-\d+))?\.config\.toml$/;
const INSTANCE_MARKER_RE = /^# codeterm-instance: ([A-Za-z0-9_-]+)$/;

export function codexProfileTabId(fileName: string): string | null {
  return CODEX_PROFILE_NAME_RE.exec(fileName)?.[1] ?? null;
}

export function codexProfileInstanceId(contents: string): string | null {
  const firstLine = contents.replace(/^\uFEFF/, "").split(/\r?\n/, 1)[0];
  return INSTANCE_MARKER_RE.exec(firstLine)?.[1] ?? null;
}

function profilePath(host: ProfileSweepHost, name: string): string | null {
  if (host.platform() !== "windows") return null;
  const home = host.homeDir();
  return home ? `${home}\\.codex\\${name}` : null;
}

function profiles(host: ProfileSweepHost): string[] | null {
  if (host.platform() !== "windows") return null;
  const home = host.homeDir();
  if (!home) return null;
  const dir = `${home}\\.codex`;
  try {
    const entries = host.fs.readDir(dir);
    if (!Array.isArray(entries)) return null;
    return entries.filter((entry) => entry?.isFile).map((entry) => entry.name);
  } catch {
    return null;
  }
}

function readProfile(host: ProfileSweepHost, path: string): string | null {
  try {
    if (host.fs.isSymlink(path) !== false) return null;
    return host.fs.readFile(path);
  } catch {
    return null;
  }
}

function removeProfile(host: ProfileSweepHost, path: string): boolean {
  try {
    if (host.fs.isSymlink(path) !== false) return false;
    return host.fs.removeFile(path);
  } catch {
    return false;
  }
}

function removeProfileAfterTrustCarry(host: ProfileSweepHost, path: string): boolean {
  const home = host.homeDir();
  if (!home) return false;
  const original = readProfile(host, path);
  if (original === null) return false;
  const configPath = `${home}\\.codex\\config.toml`;
  if (!carryCodexProjectTrust(host, path, configPath)) return false;
  return readProfile(host, path) === original && removeProfile(host, path);
}

export function inspectCodexProfiles(
  host: ProfileSweepHost,
  liveTabIds: readonly string[],
  instanceId: string,
): Pick<ProfileSweepReport, "dead" | "legacy"> {
  const listing = profiles(host);
  if (!listing) return { dead: [], legacy: [] };
  const live = new Set(liveTabIds);
  const dead: string[] = [];
  const legacy: string[] = [];
  for (const name of listing) {
    const tabId = codexProfileTabId(name);
    if (tabId === null) continue;
    const path = profilePath(host, name);
    if (!path) continue;
    const contents = readProfile(host, path);
    if (contents === null) continue;
    const owner = codexProfileInstanceId(contents);
    if (owner === instanceId && !live.has(tabId)) dead.push(name);
    else if (owner === null) legacy.push(name);
  }
  return { dead, legacy };
}

export function sweepCodexProfiles(
  host: ProfileSweepHost,
  liveTabIds: readonly string[],
  instanceId: string,
): ProfileSweepReport {
  return manageCodexProfiles(host, liveTabIds, instanceId, true, false);
}

export function manageCodexProfiles(
  host: ProfileSweepHost,
  liveTabIds: readonly string[],
  instanceId: string,
  confirm: boolean,
  includeLegacy: boolean,
): ProfileSweepReport {
  const candidates = inspectCodexProfiles(host, liveTabIds, instanceId);
  const removed: string[] = [];
  if (!confirm) return { ...candidates, removed };
  for (const name of [...candidates.dead, ...(includeLegacy ? candidates.legacy : [])]) {
    const path = profilePath(host, name);
    if (!path) continue;
    const contents = readProfile(host, path);
    if (contents === null) continue;
    const owner = codexProfileInstanceId(contents);
    const stillOwned = owner === instanceId;
    const stillLegacy = includeLegacy && owner === null;
    if ((stillOwned || stillLegacy) && removeProfileAfterTrustCarry(host, path)) removed.push(name);
  }
  return { ...candidates, removed };
}

export function releaseCodexProfile(
  host: ProfileSweepHost,
  tabId: string,
  instanceId: string,
): boolean {
  const suffix = tabId.replace(/[^A-Za-z0-9_-]/g, "-");
  const listing = profiles(host);
  if (!listing) return false;
  const ownedPaths = listing
    .filter((name) => codexProfileTabId(name) === suffix)
    .map((name) => profilePath(host, name))
    .filter((path): path is string => path !== null)
    .filter((path) => codexProfileInstanceId(readProfile(host, path) || "") === instanceId);
  if (ownedPaths.length === 0) return false;
  let released = true;
  for (const { path } of ownedPaths) {
    if (!removeProfileAfterTrustCarry(host, path)) released = false;
  }
  return released;
}
