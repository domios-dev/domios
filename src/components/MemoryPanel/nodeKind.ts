import type { MemNode, NodeKind } from "./types";

const IMAGE_MD_RE = /!\[[^\]]*\]\([^)]+\)/;

export function deriveNodeKind(node: MemNode): NodeKind {
  const tags = node.tags ?? [];
  if (tags.includes("image") || (node.body && IMAGE_MD_RE.test(node.body))) {
    return "image";
  }
  const importance = node.importance ?? 0;
  if (
    node.has_body &&
    (node.kind === "skill" || node.kind === "note" || importance > 7.0)
  ) {
    return "expanded";
  }
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
