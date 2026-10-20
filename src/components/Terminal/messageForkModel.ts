type ForkRow = { id: string; toolFlowId?: string | null };

export function forkableTurnEnds(rows: readonly ForkRow[], supportsTurnFork: boolean, hasNewerRows = false): Set<string> {
  const result = new Set<string>();
  if (!supportsTurnFork || hasNewerRows) return result;
  const turns = new Set<string>();
  for (let index = rows.length - 1; index >= 0; index--) {
    const row = rows[index];
    if (!row.toolFlowId || turns.has(row.toolFlowId)) continue;
    turns.add(row.toolFlowId);
    result.add(row.id);
  }
  return result;
}
