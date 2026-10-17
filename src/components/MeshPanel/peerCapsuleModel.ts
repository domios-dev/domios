import type { InventoryRow, InventoryState } from "./meshInventoryModel";
import type { PeerInventory } from "./peerInventoryModel";

const STATE_COLORS: Record<InventoryState, string> = {
  healthy: "var(--ct-green)", idle: "var(--ct-green)", running: "var(--ct-accent)",
  degraded: "var(--ct-yellow)", down: "var(--ct-red)", failed: "var(--ct-red)",
  unknown: "var(--ct-muted)", disabled: "var(--ct-muted)",
};

export function peerCapsuleModel(rows: InventoryRow[], state: PeerInventory["state"], width = 330) {
  const capsules = rows.map((row) => ({
    row, width: Math.min(width, 30 + Math.min(18, row.resource.name.length) * 7),
    color: STATE_COLORS[row.state], muted: state !== "ready" || row.freshness !== "fresh",
  }));
  const pack = (items: typeof capsules, more: number) => {
    const lines: typeof capsules[] = [[]];
    let used = 0;
    for (const item of items) {
      if (used && used + 4 + item.width > width) { lines.push([]); used = 0; }
      lines[lines.length - 1].push(item);
      used += (used ? 4 : 0) + item.width;
    }
    const moreWidth = Math.min(width, 24 + String(more).length * 7 + 35);
    if (more && used && used + 4 + moreWidth > width) lines.push([]);
    return { lines, moreWidth };
  };
  let visible = capsules.length;
  while (visible > 0 && pack(capsules.slice(0, visible), capsules.length - visible).lines.length > 2) visible--;
  const more = capsules.length - visible;
  return { ...pack(capsules.slice(0, visible), more), more };
}
