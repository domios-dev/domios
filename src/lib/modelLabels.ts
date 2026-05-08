// Frontend mirror of src-tauri/src/agent/models.rs::canonicalize_short.
// Existing user configs may contain version-pinned ids; the picker must
// match them to the short label so the dropdown highlights correctly.

const LEGACY: Record<string, string> = {
  "claude-sonnet-4-6": "sonnet",
  "claude-opus-4-7": "opus",
  "claude-haiku-4-5-20251001": "haiku",
  "claude-haiku-4-5": "haiku",
};

export const canonicalizeShort = (id: string): string => LEGACY[id] ?? id;

export const SHORT_LABELS = ["sonnet", "opus", "haiku"] as const;
