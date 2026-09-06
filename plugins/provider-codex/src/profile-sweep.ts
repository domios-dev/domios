// A killed codex (see plugin.ts buildLaunchCommand) skips its `finally` and
// never relaunches, so its Windows profile file orphans forever; this sweeps it.

interface SweepHostFs {
  readDir(path: string): { name: string; isFile: boolean }[];
  removeFile(path: string): boolean;
}

export interface SweepHost {
  platform(): string;
  homeDir(): string | null;
  fs: SweepHostFs;
}

const CODEX_PROFILE_NAME_RE = /^codeterm-([A-Za-z0-9_-]+)\.config\.toml$/;

/**
 * Tab id encoded in a codeterm-owned Codex Windows profile filename, or
 * `null` if the name isn't one CodeTerm wrote. Never touch what we didn't
 * create.
 */
export function codexProfileTabId(fileName: string): string | null {
  const m = CODEX_PROFILE_NAME_RE.exec(fileName);
  return m ? m[1] : null;
}

/**
 * Which owned profile file names to remove: exactly those naming a tab id
 * that is NOT in `liveTabIds`. Conservative by construction — a name that
 * doesn't match the owned-profile shape, or that names a live tab, is never
 * included.
 */
export function codexProfilesToSweep(
  fileNames: readonly string[],
  liveTabIds: ReadonlySet<string>,
): string[] {
  const out: string[] = [];
  for (const name of fileNames) {
    const tabId = codexProfileTabId(name);
    if (tabId === null) continue;
    if (!liveTabIds.has(tabId)) out.push(name);
  }
  return out;
}

/**
 * Sweep `~/.codex/codeterm-*.config.toml`: remove every owned profile whose
 * tab id is not in `liveTabIds`. Windows-only — the profile file is a
 * Windows-only launch artefact. Any failure to read the directory is treated
 * as "nothing to sweep" — never as "sweep everything" — so an unreadable
 * `~/.codex` leaves every file in place rather than risking a wrong deletion.
 */
export function sweepCodexProfiles(host: SweepHost, liveTabIds: readonly string[]): { removed: string[] } {
  if (host.platform() !== "windows") return { removed: [] };
  const home = host.homeDir();
  if (!home) return { removed: [] };
  const dir = home + "\\.codex";
  let entries: { name: string; isFile: boolean }[];
  try {
    entries = host.fs.readDir(dir);
  } catch {
    return { removed: [] };
  }
  if (!Array.isArray(entries)) return { removed: [] };
  const names = entries.filter((e) => e && e.isFile).map((e) => e.name);
  const toRemove = codexProfilesToSweep(names, new Set(liveTabIds));
  const removed: string[] = [];
  for (const name of toRemove) {
    if (host.fs.removeFile(dir + "\\" + name)) removed.push(name);
  }
  return { removed };
}
