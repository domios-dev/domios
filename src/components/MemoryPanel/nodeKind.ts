import type { MemNode, NodeKind } from "./types";

export function deriveNodeKind(_node: MemNode): NodeKind {
  return "basic";
}

export function parseFirstImage(body: string): { alt: string; url: string } | null {
  const m = body.match(/!\[([^\]]*)\]\(([^)]+)\)/);
  if (!m) return null;
  return { alt: m[1], url: m[2] };
}

export const KIND_COLORS: Record<string, string> = {
  skill: "#6c8cc7",
  fact: "#6aa88a",
  note: "#8a8e96",
  learning: "#c2a572",
  rule: "#c87c7c",
  preference: "#a084b8",
  conversation: "#6fa9a6",
  script: "#c49070",
  peer: "#b87fa4",
  daily: "#78a7b8",
  summary: "#8a8ec4",
};

export function colorForKind(kind: string): string {
  return KIND_COLORS[kind] ?? "#8a8e96";
}
