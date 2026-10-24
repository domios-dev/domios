import type { PipelineViewEdge, PipelineViewNode } from "./pipelineModel";

export const PIPELINE_CARD_WIDTH = 232;
export const PIPELINE_CARD_HEIGHT = 164;
export const PIPELINE_FLOW_GAP = 72;
export const PIPELINE_CANVAS_HEIGHT = 220;
export interface PipelineChainLayout {
  nodes: readonly { key: string; x: number; y: number; width: number; height: number }[];
  edges: readonly { from: string; to: string; path: string; x: number; y: number; label: string | null }[];
  width: number;
  height: number;
}
export function layoutPipelineChain(nodes: readonly PipelineViewNode[], routes: readonly PipelineViewEdge[]): PipelineChainLayout {
  const pad = 24;
  const columns = Math.min(4, Math.max(1, nodes.length));
  const rows = new Map(nodes.map((node, index) => [node.key, Math.floor(index / columns)]));
  const labelsPerRow = new Map<number, number>();
  for (const edge of routes) {
    const row = rows.get(edge.from);
    if (edge.label && row !== undefined) labelsPerRow.set(row, (labelsPerRow.get(row) ?? 0) + 1);
  }
  const rowGap = Math.max(PIPELINE_FLOW_GAP, 36 + Math.max(0, ...labelsPerRow.values()) * 24);
  const labelSlots = new Map<string, number>();
  const laidOut = nodes.map((node, index) => ({
    key: node.key, x: pad + (index % columns) * (PIPELINE_CARD_WIDTH + PIPELINE_FLOW_GAP),
    y: pad + Math.floor(index / columns) * (PIPELINE_CARD_HEIGHT + rowGap),
    width: PIPELINE_CARD_WIDTH, height: PIPELINE_CARD_HEIGHT,
  }));
  const byKey = new Map(laidOut.map((box) => [box.key, box]));
  return {
    nodes: laidOut,
    edges: routes.flatMap((edge, index) => {
      const from = byKey.get(edge.from), to = byKey.get(edge.to);
      if (!from || !to) return [];
      const x1 = from.x + from.width / 2, y1 = from.y + from.height;
      const x2 = to.x + to.width / 2, y2 = to.y;
      const lane = 12 + (index % 4) * 10;
      const middle = y1 + lane;
      const x = (x1 + x2) / 2;
      const slotKey = JSON.stringify([x, y1]);
      const slot = labelSlots.get(slotKey) ?? 0;
      if (edge.label) labelSlots.set(slotKey, slot + 1);
      return [{ ...edge, path: `M ${x1} ${y1} C ${x1} ${middle}, ${x2} ${middle}, ${x2} ${y2}`, x, y: y1 + 12 + slot * 24 }];
    }),
    width: pad * 2 + columns * PIPELINE_CARD_WIDTH + (columns - 1) * PIPELINE_FLOW_GAP,
    height: Math.max(PIPELINE_CANVAS_HEIGHT, pad * 2 + Math.ceil(nodes.length / columns) * (PIPELINE_CARD_HEIGHT + rowGap)),
  };
}
