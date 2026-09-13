import type { ToolRichResult } from "./types";

export type ThumbnailSource =
  | { kind: "local"; path: string }
  | { kind: "none"; reason: "missing" | "remote" | "relative" | "unsupported" };

const TEXT_FIELDS = ["query", "action", "url", "pattern", "path", "status", "prompt"] as const;
const IMAGE_FILE = /\.(?:png|jpe?g|gif|webp|bmp|avif)$/i;

/** Keeps only well-typed fields; anything else is dropped rather than coerced. */
export function normalizeToolRichResult(value: unknown): ToolRichResult | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const raw = value as Record<string, unknown>;
  if (raw.kind !== "web" && raw.kind !== "image") return undefined;
  const result: ToolRichResult = { kind: raw.kind };
  for (const field of TEXT_FIELDS) {
    const text = raw[field];
    if (typeof text === "string" && text.trim()) result[field] = text;
  }
  if (raw.origin === "generated" || raw.origin === "viewed") result.origin = raw.origin;
  if (typeof raw.resultCount === "number" && Number.isInteger(raw.resultCount) && raw.resultCount >= 0) {
    result.resultCount = raw.resultCount;
  }
  return result;
}

function fileUriPath(value: string): string | null {
  const match = /^file:\/\/([^/]*)(\/.*)$/i.exec(value);
  if (!match || (match[1] && match[1].toLowerCase() !== "localhost")) return null;
  try {
    const path = decodeURIComponent(match[2]);
    return /^\/[a-z]:[\\/]/i.test(path) ? path.slice(1) : path;
  } catch {
    return null;
  }
}

/** A thumbnail may only come from an absolute local image file; URLs and network shares are never read. */
export function localThumbnailSource(path: string | undefined): ThumbnailSource {
  const trimmed = path?.trim() ?? "";
  if (!trimmed) return { kind: "none", reason: "missing" };
  const value = /^file:/i.test(trimmed) ? fileUriPath(trimmed) : trimmed;
  if (value === null || /^[a-z][a-z0-9+.-]+:/i.test(value) || /^(?:\\\\|\/\/)/.test(value)) return { kind: "none", reason: "remote" };
  if (!/^\//.test(value) && !/^[a-z]:[\\/]/i.test(value)) return { kind: "none", reason: "relative" };
  if (!IMAGE_FILE.test(value)) return { kind: "none", reason: "unsupported" };
  return { kind: "local", path: value };
}
