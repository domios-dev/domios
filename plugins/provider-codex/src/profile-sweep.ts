import { carryCodexProjectTrust, hasTrustedProjectEntries, projectTrustForPath } from "./trust-carryover";

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

function codexConfigPath(host: ProfileSweepHost, home: string): string {
  const separator = host.platform() === "windows" ? "\\" : "/";
  return `${home}${separator}.codex${separator}config.toml`;
}

export function isCodexDirectoryTrusted(host: ProfileSweepHost, path: string): boolean {
  return codexDirectoryTrustState(host, path) === "trusted";
}

export function codexDirectoryTrustState(
  host: ProfileSweepHost,
  cwd: string,
  _skipPermissions = false,
): "trusted" | "untrusted" | "unknown" {
  const home = host.homeDir();
  if (!home || !isAbsolutePath(host, cwd)) return "unknown";
  const configPath = codexConfigPath(host, home);
  const configSymlink = host.fs.isSymlink(configPath);
  if (configSymlink !== false) return "unknown";
  const configExists = host.fs.fileExists(configPath);
  const config = configExists ? host.fs.readFile(configPath) : "";
  if (config === null) return "unknown";
  const markers = projectRootMarkers(config);
  if (!markers) return "unknown";
  const roots = codexTrustRoots(host, cwd, markers);
  if (!roots) return "unknown";
  for (const root of roots) {
    const trust = projectTrustForPath(config, root);
    if (trust === "trusted") return "trusted";
    if (trust === "untrusted") return "untrusted";
    if (trust === "unknown") return "unknown";
  }
  return "untrusted";
}

function isAbsolutePath(host: ProfileSweepHost, path: string): boolean {
  return host.platform() === "windows"
    ? /^[A-Za-z]:[\\/]/.test(path) || /^\\\\/.test(path)
    : path.startsWith("/");
}

function projectRootMarkers(config: string): string[] | null {
  const lines = config.split(/\r?\n/);
  const assignments = lines.filter((line) => /^\s*project_root_markers\s*=/.test(line));
  if (assignments.length === 0) return [".git"];
  if (assignments.length !== 1) return null;
  const value = assignments[0].slice(assignments[0].indexOf("=") + 1).trim();
  if (!value.startsWith("[") || !value.endsWith("]")) return null;
  const body = value.slice(1, -1).trim();
  if (!body) return [];
  const markers: string[] = [];
  let rest = body;
  while (rest) {
    const match = /^("(?:[^"\\]|\\.)*"|'[^']*')\s*(?:,\s*|$)/.exec(rest);
    if (!match) return null;
    const parsed = match[1][0] === '"'
      ? (() => { try { return JSON.parse(match[1]) as string; } catch { return null; } })()
      : match[1].slice(1, -1);
    if (parsed === null || parsed.length === 0) return null;
    markers.push(parsed);
    rest = rest.slice(match[0].length);
  }
  return markers;
}

function codexTrustRoots(host: ProfileSweepHost, cwd: string, markers: string[]): string[] | null {
  const windows = host.platform() === "windows";
  const normalizedCwd = normalizeAbsolutePath(cwd, windows);
  if (!normalizedCwd) return null;
  const projectRoot = markers.length === 0 ? normalizedCwd : findRoot(host, normalizedCwd, markers, windows);
  const gitRoot = findRoot(host, normalizedCwd, [".git"], windows);
  if (projectRoot === null || gitRoot === null) return null;
  const mainRepoRoot = gitRoot === undefined ? undefined : worktreeMainRoot(host, gitRoot, windows);
  return uniquePaths(
    [normalizedCwd, projectRoot, gitRoot, mainRepoRoot].filter((path): path is string => path !== undefined),
    windows,
  );
}

/** A linked worktree's `.git` file names `<main>/.git/worktrees/<name>`; Codex trusts it through `<main>`. */
function worktreeMainRoot(host: ProfileSweepHost, gitRoot: string, windows: boolean): string | undefined {
  const separator = windows ? "\\" : "/";
  const pointer = host.fs.readFile(`${gitRoot.replace(/[\\/]$/, "")}${separator}.git`);
  const gitdir = pointer?.match(/^gitdir:\s*(.+?)\s*$/m)?.[1];
  if (!gitdir) return undefined;
  const absolute = isAbsolutePath(host, gitdir) ? gitdir : `${gitRoot}${separator}${gitdir}`;
  const normalized = normalizeAbsolutePath(absolute, windows);
  const main = normalized?.match(/^(.*?)[\\/]\.git[\\/]worktrees[\\/][^\\/]+[\\/]?$/i)?.[1];
  return main || undefined;
}

function findRoot(
  host: ProfileSweepHost,
  cwd: string,
  markers: string[],
  windows: boolean,
): string | undefined | null {
  const separator = windows ? "\\" : "/";
  let current = cwd;
  const root = windows ? current.slice(0, 3) : "/";
  while (current) {
    if (host.fs.isSymlink(current) !== false) return null;
    for (const markerName of markers) {
      const marker = `${current.replace(/[\\/]$/, "")}${separator}${markerName}`;
      const markerSymlink = host.fs.isSymlink(marker);
      if (markerSymlink === null || markerSymlink === true) return null;
      if (host.fs.fileExists(marker)) return current;
    }
    if (current === root) break;
    const parent = parentPath(current, windows);
    if (!parent || parent === current) break;
    current = parent;
  }
  return markers.length === 0 ? cwd : undefined;
}

function normalizeAbsolutePath(path: string, windows: boolean): string | null {
  const value = windows ? path.replace(/\//g, "\\") : path;
  const prefix = windows ? /^([A-Za-z]:\\|\\\\[^\\]+\\[^\\]+\\)/.exec(value)?.[0] : "/";
  if (!prefix) return null;
  const tail = value.slice(prefix.length);
  const parts: string[] = [];
  for (const part of tail.split(/[\\/]+/)) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (parts.length === 0) return null;
      parts.pop();
    } else {
      parts.push(part);
    }
  }
  const joined = `${prefix}${parts.join(windows ? "\\" : "/")}`;
  return joined;
}

function parentPath(path: string, windows: boolean): string | null {
  const separator = windows ? "\\" : "/";
  const normalized = path.replace(/[\\/]$/, "");
  const index = normalized.lastIndexOf(separator);
  if (index < 0) return null;
  if (windows && index === 2) return normalized.slice(0, 3);
  if (!windows && index === 0) return "/";
  return normalized.slice(0, index);
}

function uniquePaths(paths: string[], windows: boolean): string[] {
  const seen = new Set<string>();
  return paths.filter((path) => {
    const key = windows ? path.toLowerCase() : path;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function codexProfileTabId(fileName: string): string | null {
  return CODEX_PROFILE_NAME_RE.exec(fileName)?.[1] ?? null;
}

export function codexProfileInstanceId(contents: string): string | null {
  const firstLine = contents.replace(/^\uFEFF/, "").split(/\r?\n/, 1)[0];
  return INSTANCE_MARKER_RE.exec(firstLine)?.[1] ?? null;
}

function profilePath(host: ProfileSweepHost, name: string): string | null {
  const home = host.homeDir();
  const separator = host.platform() === "windows" ? "\\" : "/";
  return home ? `${home}${separator}.codex${separator}${name}` : null;
}

function profiles(host: ProfileSweepHost): string[] | null {
  const home = host.homeDir();
  if (!home) return null;
  const separator = host.platform() === "windows" ? "\\" : "/";
  const dir = `${home}${separator}.codex`;
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
  const configPath = codexConfigPath(host, home);
  if (!carryCodexProjectTrust(host, path, configPath)) return false;
  return readProfile(host, path) === original && removeProfile(host, path);
}

export function carryCodexProjectTrustForTab(
  host: ProfileSweepHost,
  tabId: string,
  instanceId: string,
): boolean {
  const listing = profiles(host);
  const home = host.homeDir();
  if (!listing || !home) return false;
  const configPath = codexConfigPath(host, home);
  const suffix = tabId.replace(/[^A-Za-z0-9_-]/g, "-");
  const owned = listing
    .filter((name) => codexProfileTabId(name) === suffix)
    .map((name) => profilePath(host, name))
    .filter((path): path is string => path !== null)
    .filter((path) => codexProfileInstanceId(readProfile(host, path) || "") === instanceId);
  if (owned.length === 0) return false;
  let foundTrustedEntry = false;
  for (const path of owned) {
    const profile = readProfile(host, path);
    if (profile === null) return false;
    if (!hasTrustedProjectEntries(profile)) continue;
    foundTrustedEntry = true;
    if (!carryCodexProjectTrust(host, path, configPath)) return false;
  }
  return foundTrustedEntry;
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
  for (const path of ownedPaths) {
    if (!removeProfileAfterTrustCarry(host, path)) released = false;
  }
  return released;
}
