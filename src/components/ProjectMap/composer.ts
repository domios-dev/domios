import type { ProjectMapEdge, ProjectMapNode } from "./types";

export interface ComposeInput {
  prompt: string;
  selectedIds: number[];
  nodesById: Map<number, ProjectMapNode>;
  focusId: number | null;
  edges: ProjectMapEdge[];
}

export function buildMessage(input: ComposeInput): string {
  const lines: string[] = [];
  if (input.prompt.trim()) {
    lines.push(input.prompt.trim());
    lines.push("");
  }
  if (input.selectedIds.length > 0) {
    lines.push(`--- Selected nodes (${input.selectedIds.length}) ---`);
    for (const id of input.selectedIds) {
      const n = input.nodesById.get(id);
      if (!n) continue;
      lines.push(formatNode(n));
    }
  }
  if (input.focusId != null && !input.selectedIds.includes(input.focusId)) {
    const n = input.nodesById.get(input.focusId);
    if (n) {
      lines.push("");
      lines.push("--- Focus context ---");
      lines.push(`Currently focused: ${formatNodeInline(n)}`);
    }
  }
  if (input.selectedIds.length > 1) {
    const summary = summariseEdgesWithin(new Set(input.selectedIds), input.edges);
    if (summary) {
      lines.push("");
      lines.push(`Edges between selection: ${summary}`);
    }
  }
  return lines.join("\n");
}

function formatNode(n: ProjectMapNode): string {
  const loc = n.path && n.start_line != null ? `${n.path}:${n.start_line + 1}` : n.path ?? "-";
  return `- ${loc} — ${n.display_name} (${n.kind})`;
}

function formatNodeInline(n: ProjectMapNode): string {
  const loc = n.path && n.start_line != null ? `${n.path}:${n.start_line + 1}` : n.path ?? "-";
  return `${loc} — ${n.display_name} (${n.kind})`;
}

function summariseEdgesWithin(ids: Set<number>, edges: ProjectMapEdge[]): string {
  const counts: Record<string, number> = {};
  for (const e of edges) {
    if (ids.has(e.source) && ids.has(e.target)) {
      counts[e.kind] = (counts[e.kind] ?? 0) + 1;
    }
  }
  const parts = Object.entries(counts).map(([k, v]) => `${v} ${k}`);
  return parts.join(", ");
}
