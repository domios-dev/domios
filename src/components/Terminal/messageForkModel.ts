type ForkRow = { id: string; toolFlowId?: string | null };

const ACP_TURN_ID = /^(.+:turn:\d+)(?::|$)/;

export function messageTurnId(row: ForkRow): string | undefined {
  if (row.toolFlowId) return row.toolFlowId;
  return ACP_TURN_ID.exec(row.id)?.[1];
}

export function forkableTurnEnds(rows: readonly ForkRow[], supportsTurnFork: boolean, hasNewerRows = false): Set<string> {
  const result = new Set<string>();
  if (!supportsTurnFork || hasNewerRows) return result;
  const turns = new Set<string>();
  for (let index = rows.length - 1; index >= 0; index--) {
    const row = rows[index];
    const turnId = messageTurnId(row);
    if (!turnId || turns.has(turnId)) continue;
    turns.add(turnId);
    result.add(row.id);
  }
  return result;
}
