export const MODEL_BADGE_MAX_CHARS = 18;

export const MODEL_BADGE_SUFFIXES = [
  "QAT",
  "A4B",
  "A3B",
  "MLX",
  "GGUF",
  "4BIT",
  "8BIT",
  "INSTRUCT",
  "IT",
] as const;

const SUFFIX_PATTERN = new RegExp(`-(${MODEL_BADGE_SUFFIXES.join("|")})$`, "i");

export function formatModelBadge(modelId: string, maxChars = MODEL_BADGE_MAX_CHARS): string {
  const normalized = modelId.trim();
  if (normalized.length <= maxChars) return normalized;

  const withoutVendor = normalized.replace(/^[^/]+\//, "");
  if (withoutVendor.length <= maxChars) return withoutVendor;

  let compact = withoutVendor;
  while (SUFFIX_PATTERN.test(compact)) {
    compact = compact.replace(SUFFIX_PATTERN, "");
  }

  if (compact.length <= maxChars) return compact;
  return ellipsizeEnd(compact, maxChars);
}

function ellipsizeEnd(value: string, maxChars: number): string {
  if (maxChars <= 3) return value.slice(0, maxChars);
  return `${value.slice(0, maxChars - 3)}...`;
}
