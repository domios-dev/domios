export interface DropRow {
  id: string;
  top: number;
  bottom: number;
}

export function sidebarDropTarget(rows: readonly DropRow[], activeId: string, y: number): string | null {
  const from = rows.findIndex(row => row.id === activeId);
  if (from < 0) return null;
  const remaining = rows.filter(row => row.id !== activeId);
  const before = remaining.findIndex(row => y < (row.top + row.bottom) / 2);
  const to = before < 0 ? remaining.length : before;
  return rows[to]?.id ?? activeId;
}

export function sidebarDropEdge(items: readonly string[], activeId: string, overId: string): "top" | "bottom" | null {
  const from = items.indexOf(activeId);
  const to = items.indexOf(overId);
  if (from < 0 || to < 0 || from === to) return null;
  return to < from ? "top" : "bottom";
}
