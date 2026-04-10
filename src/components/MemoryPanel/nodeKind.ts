import type { MemNode, NodeKind } from "./types";

export function deriveNodeKind(_node: MemNode): NodeKind {
  // All nodes render as dots on the graph canvas.
  // Expanded/image renderers are kept for future use but not active.
  return "basic";
}

export function parseFirstImage(body: string): { alt: string; url: string } | null {
  const m = body.match(/!\[([^\]]*)\]\(([^)]+)\)/);
  if (!m) return null;
  return { alt: m[1], url: m[2] };
}

export const KIND_COLORS: Record<string, string> = {
  skill: "#3b82f6",
  fact: "#10b981",
  note: "#9ca3af",
  learning: "#eab308",
  rule: "#ef4444",
  preference: "#a855f7",
  conversation: "#14b8a6",
  script: "#f97316",
  peer: "#ec4899",
  daily: "#06b6d4",
  summary: "#6366f1",
};

export function colorForKind(kind: string): string {
  return KIND_COLORS[kind] ?? "#ffffff";
}
