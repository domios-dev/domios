import type { BackgroundProcessEntry } from "./backgroundProcesses";

export interface BackgroundProcessMember {
  command: string;
  osPid?: number | null;
  role: "main" | "helper";
  processId?: string;
  itemId?: string;
}

export interface BackgroundProcessGroup {
  primary: BackgroundProcessEntry;
  members: BackgroundProcessMember[];
  helperCount: number;
  processIds: string[];
  itemIds: string[];
}

const PLUMBING_EXECUTABLES = new Set([
  "tee",
  "cat",
  "head",
  "tail",
  "sccache",
  "sccache-dist",
  "ccache",
  "buildcache",
]);

const BARE_SHELL_EXECUTABLES = new Set(["sh", "bash", "zsh", "dash", "pwsh", "powershell", "cmd"]);

function executableStem(raw: string): string {
  const unquoted = raw.replace(/^["']|["']$/g, "");
  const leaf = unquoted.split(/[\\/]/).pop() ?? unquoted;
  return leaf.replace(/\.(?:exe|cmd|bat)$/i, "").toLowerCase();
}

export function isPlumbingHelperCommand(command: string): boolean {
  const trimmed = command.trim();
  if (!trimmed) return false;
  const first = trimmed.match(/^(?:"([^"]+)"|'([^']+)'|(\S+))/);
  if (!first) return false;
  const head = executableStem(first[1] ?? first[2] ?? first[3] ?? "");
  if (PLUMBING_EXECUTABLES.has(head)) return true;
  if (!BARE_SHELL_EXECUTABLES.has(head)) return false;
  const rest = trimmed.slice(first[0].length).trim();
  if (!rest) return true;
  return /^(?:-(?:l|i|-login|-noprofile|-norc|nologo|noexit|noninteractive|sta|mta)|\/[qduak])(?:\s+(?:-(?:l|i|-login|-noprofile|-norc|nologo|noexit|noninteractive|sta|mta)|\/[qduak]))*$/i.test(rest);
}

/**
 * Group live background processes into one entry per top-level command or process tree.
 * Grouping uses structural signals only (shared launching itemId, OS parent->child ancestry,
 * backend-attached tree children, or shared non-root pipeline parent with a plumbing helper),
 * never name guesses alone.
 */
export function groupBackgroundProcesses(entries: readonly BackgroundProcessEntry[]): BackgroundProcessGroup[] {
  const uniqueMap = new Map<string, BackgroundProcessEntry>();
  for (const entry of entries) {
    uniqueMap.set(JSON.stringify([entry.threadId, entry.processId]), entry);
  }
  const unique = [...uniqueMap.values()];
  if (unique.length === 0) return [];

  const parent = unique.map((_, index) => index);
  const find = (index: number): number => {
    let root = index;
    while (parent[root] !== root) root = parent[root];
    let current = index;
    while (parent[current] !== root) {
      const next = parent[current];
      parent[current] = root;
      current = next;
    }
    return root;
  };
  const union = (a: number, b: number) => {
    const rootA = find(a);
    const rootB = find(b);
    if (rootA === rootB) return;
    if (rootA < rootB) parent[rootB] = rootA;
    else parent[rootA] = rootB;
  };

  // Index live OS pids (entries and their already-attached children) scoped by threadId.
  const indexByThreadPid = new Map<string, number>();
  unique.forEach((entry, index) => {
    if (typeof entry.osPid === "number" && entry.osPid > 1) {
      indexByThreadPid.set(JSON.stringify([entry.threadId, entry.osPid]), index);
    }
  });
  unique.forEach((entry, index) => {
    for (const child of entry.children ?? []) {
      if (typeof child.osPid === "number" && child.osPid > 1) {
        const key = JSON.stringify([entry.threadId, child.osPid]);
        if (!indexByThreadPid.has(key)) indexByThreadPid.set(key, index);
      }
    }
  });

  // 1. Same launching tool/task id within the same thread.
  const firstByItem = new Map<string, number>();
  unique.forEach((entry, index) => {
    if (!entry.itemId) return;
    const key = JSON.stringify([entry.threadId, entry.itemId]);
    const existing = firstByItem.get(key);
    if (existing !== undefined) union(existing, index);
    else firstByItem.set(key, index);
  });

  // 2. Direct/transitive OS process tree ancestry (`parentOsPid` matches a live entry or child `osPid`).
  unique.forEach((entry, index) => {
    if (typeof entry.parentOsPid !== "number" || entry.parentOsPid <= 1) return;
    const owner = indexByThreadPid.get(JSON.stringify([entry.threadId, entry.parentOsPid]));
    if (owner !== undefined && owner !== index) union(owner, index);
  });

  // 3. Siblings sharing an exited pipeline parent in the same cwd when plumbing joins them.
  const bySharedDeadParent = new Map<string, number[]>();
  unique.forEach((entry, index) => {
    if (typeof entry.parentOsPid !== "number" || entry.parentOsPid <= 1) return;
    if (indexByThreadPid.has(JSON.stringify([entry.threadId, entry.parentOsPid]))) return;
    const key = JSON.stringify([entry.threadId, entry.parentOsPid, entry.cwd]);
    const bucket = bySharedDeadParent.get(key);
    if (bucket) bucket.push(index);
    else bySharedDeadParent.set(key, [index]);
  });
  for (const bucket of bySharedDeadParent.values()) {
    if (bucket.length < 2) continue;
    if (!bucket.some((index) => isPlumbingHelperCommand(unique[index].command))) continue;
    const [lead, ...rest] = bucket;
    for (const sibling of rest) union(lead, sibling);
  }

  const components = new Map<number, number[]>();
  unique.forEach((_, index) => {
    const root = find(index);
    const list = components.get(root);
    if (list) list.push(index);
    else components.set(root, [index]);
  });

  const groups: BackgroundProcessGroup[] = [];
  for (const memberIndices of components.values()) {
    const memberPids = new Set<number>();
    for (const index of memberIndices) {
      const pid = unique[index].osPid;
      if (typeof pid === "number" && pid > 1) memberPids.add(pid);
      for (const child of unique[index].children ?? []) {
        if (typeof child.osPid === "number" && child.osPid > 1) memberPids.add(child.osPid);
      }
    }
    const componentRoots = memberIndices.filter((index) => {
      const parentPid = unique[index].parentOsPid;
      return typeof parentPid !== "number" || !memberPids.has(parentPid);
    });
    const candidates = componentRoots.length > 0 ? componentRoots : memberIndices;
    const primaryIndex = candidates.find((index) => !isPlumbingHelperCommand(unique[index].command)) ?? candidates[0];
    const primary = unique[primaryIndex];

    const seenPids = new Set<number>();
    const members: BackgroundProcessMember[] = [{
      command: primary.command,
      osPid: primary.osPid,
      role: "main",
      processId: primary.processId,
      itemId: primary.itemId,
    }];
    if (typeof primary.osPid === "number" && primary.osPid > 1) seenPids.add(primary.osPid);

    for (const index of memberIndices) {
      if (index === primaryIndex) continue;
      const entry = unique[index];
      if (typeof entry.osPid === "number" && entry.osPid > 1) {
        if (seenPids.has(entry.osPid)) continue;
        seenPids.add(entry.osPid);
      }
      members.push({
        command: entry.command,
        osPid: entry.osPid,
        role: "helper",
        processId: entry.processId,
        itemId: entry.itemId,
      });
    }

    const ordered = [primaryIndex, ...memberIndices.filter((index) => index !== primaryIndex)];
    for (const index of ordered) {
      for (const child of unique[index].children ?? []) {
        if (typeof child.osPid === "number" && child.osPid > 1) {
          if (seenPids.has(child.osPid)) continue;
          seenPids.add(child.osPid);
        }
        members.push({
          command: child.command,
          osPid: child.osPid ?? null,
          role: "helper",
        });
      }
    }

    groups.push({
      primary,
      members,
      helperCount: Math.max(0, members.length - 1),
      processIds: memberIndices.map((index) => unique[index].processId),
      itemIds: [...new Set(memberIndices.map((index) => unique[index].itemId))],
    });
  }
  return groups;
}
