
export interface PresenceAgent {
  /** Stable identity key — same agent → same color/initials everywhere. */
  id: string;
  label: string;
  role: string | null;
  color: string;
  /** A collaborator (task.peer) rather than the lane's primary executor. */
  isPeer?: boolean;
}

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

/** Glyph colour on every presence hue; the hues are painted on canvas, where theme tokens cannot resolve. */
export const PRESENCE_INK = "#fff";

export function presenceColor(key: string): string {
  let h = 0;
  for (let i = 0; i < key.length; i++) h = (h * 31 + key.charCodeAt(i)) >>> 0;
  return PRESENCE_PALETTE[h % PRESENCE_PALETTE.length];
}

// two chars. Splits on the separators agent handles actually use.
export function agentInitials(label: string): string {
  const parts = label.trim().split(/[\s_\-./:]+/).filter(Boolean);
  if (parts.length === 0) return "··";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[1][0]).toUpperCase();
}
