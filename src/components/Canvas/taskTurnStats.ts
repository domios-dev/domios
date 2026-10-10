import type { LaneTurnChangeStats } from "./types";

export interface TurnChangeSummary {
  files: number;
  additions: number;
  deletions: number;
}

export function summarizeTurnNumstat(...numstats: Array<string | null | undefined>): TurnChangeSummary {
  const files = new Set<string>();
  let additions = 0;
  let deletions = 0;
  for (const numstat of numstats) {
    for (const line of numstat?.split(/\r?\n/) ?? []) {
      const firstTab = line.indexOf("\t");
      const secondTab = firstTab < 0 ? -1 : line.indexOf("\t", firstTab + 1);
      if (secondTab < 0) continue;
      const path = line.slice(secondTab + 1);
      if (!path) continue;
      files.add(path);
      const added = Number(line.slice(0, firstTab));
      const removed = Number(line.slice(firstTab + 1, secondTab));
      if (Number.isFinite(added)) additions += added;
      if (Number.isFinite(removed)) deletions += removed;
    }
  }
  return { files: files.size, additions, deletions };
}

export function laneTurnStatsLabel(stats: LaneTurnChangeStats): string {
  const summary = summarizeTurnNumstat(stats.numstat, stats.staged_numstat);
  const untracked = stats.untracked_file_count ?? 0;
  return `Turn ${stats.turn}: ${summary.files} files, +${summary.additions}/−${summary.deletions}, ${untracked} untracked`;
}
