// EMERGENCE (north-star #2): a swarm reads as ALIVE only if every agent keeps the
// SAME color + initials wherever it appears — on a task card now, free-floating
// between tasks (iter-11), in the top-right cluster (iter-12). These pure helpers
// are that shared identity contract; the React tokens in AgentCircle.tsx consume it.

export interface PresenceAgent {
  /** Stable identity key — same agent → same color/initials everywhere. */
  id: string;
  label: string;
  role: string | null;
  color: string;
  /** A collaborator (task.peer) rather than the lane's primary executor. */
  isPeer?: boolean;
}

// Curated multiplayer palette: ten well-separated hues, each tuned for white
// glyph contrast. A hash indexes it so identity is deterministic, never a muddy
// hash-to-hue accident. Deliberately NOT the templated terracotta/cream default.
export const PRESENCE_PALETTE = [
  "#e8643c", // vermilion
  "#2f9e7e", // teal
  "#4f7df0", // blue
  "#c54fd6", // magenta
  "#e0a52e", // amber
  "#3fb6c9", // cyan
  "#8a63e8", // violet
  "#d24d77", // rose
  "#6aa83a", // leaf
  "#d97b2b", // orange
] as const;

export function presenceColor(key: string): string {
  let h = 0;
  for (let i = 0; i < key.length; i++) h = (h * 31 + key.charCodeAt(i)) >>> 0;
  return PRESENCE_PALETTE[h % PRESENCE_PALETTE.length];
}

// Initials from a handle/role/title: two-word → first letters, one-word → first
// two chars. Splits on the separators agent handles actually use.
export function agentInitials(label: string): string {
  const parts = label.trim().split(/[\s_\-./:]+/).filter(Boolean);
  if (parts.length === 0) return "··";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[1][0]).toUpperCase();
}
