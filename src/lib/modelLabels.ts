// Keep frontend label mapping aligned with `canonicalize_short`.
// Version-pinned IDs in config should still highlight as expected.

const LEGACY: Record<string, string> = {
  "claude-sonnet-4-6": "sonnet",
  "claude-opus-4-7": "opus",
  "claude-haiku-4-5-20251001": "haiku",
  "claude-haiku-4-5": "haiku",
};

export const canonicalizeShort = (id: string): string => LEGACY[id] ?? id;

export const SHORT_LABELS = ["sonnet", "opus", "haiku"] as const;
