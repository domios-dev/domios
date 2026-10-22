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
  const laidOut = nodes.map((node, index) => ({
    key: node.key, x: pad + (index % columns) * (PIPELINE_CARD_WIDTH + PIPELINE_FLOW_GAP),
    y: pad + Math.floor(index / columns) * (PIPELINE_CARD_HEIGHT + PIPELINE_FLOW_GAP),
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
      return [{ ...edge, path: `M ${x1} ${y1} C ${x1} ${middle}, ${x2} ${middle}, ${x2} ${y2}`, x: (x1 + x2) / 2, y: middle }];
    }),
    width: pad * 2 + columns * PIPELINE_CARD_WIDTH + (columns - 1) * PIPELINE_FLOW_GAP,
    height: Math.max(PIPELINE_CANVAS_HEIGHT, pad * 2 + Math.ceil(nodes.length / columns) * (PIPELINE_CARD_HEIGHT + PIPELINE_FLOW_GAP)),
  };
}
