import type { PipelineViewNode } from "./pipelineModel";

export const PIPELINE_CARD_WIDTH = 232;
export const PIPELINE_CARD_HEIGHT = 164;
export const PIPELINE_FLOW_GAP = 72;
export const PIPELINE_CANVAS_HEIGHT = 220;

export interface PipelineChainLayout {
  nodes: readonly { key: string; x: number; y: number; width: number; height: number }[];
  edges: readonly { from: string; to: string; x: number; width: number; y: number; label: string | null }[];
  width: number;
  height: number;
}

export function layoutPipelineChain(nodes: readonly PipelineViewNode[]): PipelineChainLayout {
  const pad = 16;
  const nodeY = (PIPELINE_CANVAS_HEIGHT - PIPELINE_CARD_HEIGHT) / 2;
  const laidOut = nodes.map((node, index) => ({
    key: node.key,
    x: pad + index * (PIPELINE_CARD_WIDTH + PIPELINE_FLOW_GAP),
    y: nodeY,
    width: PIPELINE_CARD_WIDTH,
    height: PIPELINE_CARD_HEIGHT,
  }));
  return {
    nodes: laidOut,
    edges: nodes.slice(0, -1).map((node, index) => ({
      from: node.key,
      to: nodes[index + 1].key,
      x: laidOut[index].x + PIPELINE_CARD_WIDTH,
      width: PIPELINE_FLOW_GAP,
      y: PIPELINE_CANVAS_HEIGHT / 2,
      label: node.kind === "step" ? node.outcome : null,
    })),
    width: Math.max(pad * 2, pad * 2 + nodes.length * PIPELINE_CARD_WIDTH + Math.max(0, nodes.length - 1) * PIPELINE_FLOW_GAP),
    height: PIPELINE_CANVAS_HEIGHT,
  };
}
