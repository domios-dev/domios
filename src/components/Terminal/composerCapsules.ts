import { ATTACHMENT_KINDS, attachmentKindForPath, attachmentMarkerSegments, formatAttachmentMarker, hasAttachmentMarkers, type AttachmentKind } from "@codeterm/chat-core";

export interface ComposerCapsule {
  label: string;
  kind: AttachmentKind;
  path: string;
  uploadId?: number;
}

/** Capsules a draft may reference, keyed by label; entries outlive their token so undo can bring one back. */
export type CapsuleRegistry = Readonly<Record<string, ComposerCapsule>>;

export interface CapsuleSpan {
  start: number;
  end: number;
  label: string;
}

export interface CapsuleRequest {
  kind: AttachmentKind;
  path: string;
  label?: string;
  /** What a numbered quote refers to: "quote1 · <detail>". */
  detail?: string;
  lines?: number;
}

export const EMPTY_CAPSULES: CapsuleRegistry = Object.freeze({});
export const LONG_PASTE_CHARS = 2000;
export const LONG_PASTE_LINES = 40;

// A token is a bidi isolate around the label: zero-width ends nobody types, and non-breaking spaces
// reserve the icon slot (two figure + one no-break) and the trailing pad, so the painted capsule matches the text width.
const OPEN = "\u2068";
const CLOSE = "\u2069";
const PAD = "\u2007";
export const CAPSULE_LEAD = `${OPEN}${PAD}${PAD} `;
export const CAPSULE_TRAIL = `\u00A0${CLOSE}`;
const TOKEN = /\u2068\u2007\u2007\u00A0([^\u2068\u2069\n]+?)\u00A0\u2069/g;
const LABEL_MAX = 32;

/** A label as the token spells it: spaces become no-break spaces so a capsule never wraps. */
export function labelChars(label: string): string {
  return label.replace(/ /g, "\u00A0");
}

export function capsuleToken(label: string): string {
  return `${CAPSULE_LEAD}${labelChars(label)}${CAPSULE_TRAIL}`;
}

export function capsuleSpans(text: string): CapsuleSpan[] {
  return [...text.matchAll(TOKEN)].map((match) => ({
    start: match.index ?? 0,
    end: (match.index ?? 0) + match[0].length,
    label: match[1].replace(/\u00A0/g, " "),
  }));
}

export function hasCapsuleTokens(text: string): boolean {
  return text.includes(OPEN) && capsuleSpans(text).length > 0;
}

export function countLines(text: string): number {
  return text.replace(/\r?\n$/, "").split(/\r?\n/).length;
}

export function isLongPaste(text: string): boolean {
  return text.length > LONG_PASTE_CHARS || countLines(text) > LONG_PASTE_LINES;
}

function baseName(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function cleanLabel(label: string): string {
  const flat = label.replace(/[\u2068\u2069\u2007\u00A0\r\n\t]+/g, " ").replace(/\s+/g, " ").trim();
  if (flat.length <= LABEL_MAX) return flat;
  return `${flat.slice(0, LABEL_MAX - 10)}…${flat.slice(-9)}`;
}

function numberedLabel(registry: CapsuleRegistry, prefix: string, suffix = ""): string {
  const pattern = new RegExp(`^${prefix}(\\d+)(?:\\s|$)`);
  let max = 0;
  for (const label of Object.keys(registry)) {
    const n = Number(pattern.exec(label)?.[1] ?? 0);
    if (n > max) max = n;
  }
  return `${prefix}${max + 1}${suffix}`;
}

function linesSuffix(lines: number): string {
  return ` · ${lines} ${lines === 1 ? "line" : "lines"}`;
}

const NUMBERED_KINDS: ReadonlySet<AttachmentKind> = new Set(["image", "video", "text", "quote"]);

function quoteDetail(request: CapsuleRequest): string {
  const detail = request.detail ?? request.label?.split(" · ").slice(1).join(" · ") ?? "";
  return detail ? ` · ${cleanLabel(detail)}` : "";
}

function generatedLabel(registry: CapsuleRegistry, request: CapsuleRequest): string {
  if (request.kind === "image") return numberedLabel(registry, "image");
  if (request.kind === "video") return numberedLabel(registry, "video");
  if (request.kind === "text") return numberedLabel(registry, "text", request.lines ? linesSuffix(request.lines) : "");
  if (request.kind === "quote") return numberedLabel(registry, "quote", quoteDetail(request));
  return cleanLabel(baseName(request.path)) || request.kind;
}

function uniqueLabel(registry: CapsuleRegistry, label: string): string {
  if (!registry[label]) return label;
  for (let n = 2; ; n += 1) {
    const candidate = `${label} (${n})`;
    if (!registry[candidate]) return candidate;
  }
}

function samePath(a: string, b: string): boolean {
  return a.replace(/\\/g, "/") === b.replace(/\\/g, "/");
}

/** Registers a capsule for a path; the same path in one draft keeps its label. */
export function addCapsule(registry: CapsuleRegistry, request: CapsuleRequest): { registry: CapsuleRegistry; capsule: ComposerCapsule } {
  const existing = Object.values(registry).find((capsule) => capsule.uploadId === undefined && capsule.kind === request.kind && samePath(capsule.path, request.path));
  if (existing) return { registry, capsule: existing };
  const preferred = request.label ? cleanLabel(request.label) : "";
  const label = preferred && !registry[preferred]
    ? preferred
    : uniqueLabel(registry, preferred && !NUMBERED_KINDS.has(request.kind) ? preferred : generatedLabel(registry, request));
  const capsule: ComposerCapsule = { label, kind: request.kind, path: request.path };
  return { registry: { ...registry, [label]: capsule }, capsule };
}

/** Capsule tokens for paths, in order, separated by spaces. */
export function capsulesForPaths(
  registry: CapsuleRegistry,
  items: ReadonlyArray<{ path: string; kind?: AttachmentKind; label?: string; detail?: string }>,
): { registry: CapsuleRegistry; text: string } {
  let next = registry;
  const tokens: string[] = [];
  for (const item of items) {
    const added = addCapsule(next, { kind: item.kind ?? attachmentKindForPath(item.path), path: item.path, label: item.label, detail: item.detail });
    next = added.registry;
    tokens.push(capsuleToken(added.capsule.label));
  }
  return { registry: next, text: tokens.join(" ") };
}

/** Turns attachment tags (a copied draft, a recalled message) into capsule tokens. */
export function absorbAttachmentTags(text: string, registry: CapsuleRegistry): { text: string; registry: CapsuleRegistry } {
  if (!hasAttachmentMarkers(text)) return { text, registry };
  let next = registry;
  let out = "";
  for (const segment of attachmentMarkerSegments(text)) {
    if ("text" in segment) {
      out += segment.text;
      continue;
    }
    const { kind, path, label } = segment.attachment;
    const added = addCapsule(next, { kind, path, label, lines: Number(/· (\d+) lines?$/.exec(label ?? "")?.[1]) || undefined });
    next = added.registry;
    out += capsuleToken(added.capsule.label);
  }
  return { text: out, registry: next };
}

export function capsulesInDraft(text: string, registry: CapsuleRegistry): ComposerCapsule[] {
  const seen = new Set<string>();
  const capsules: ComposerCapsule[] = [];
  for (const { label } of capsuleSpans(text)) {
    const capsule = registry[label];
    if (!capsule || seen.has(label)) continue;
    seen.add(label);
    capsules.push(capsule);
  }
  return capsules;
}

export function capsuleMarker(capsule: ComposerCapsule): string {
  return capsule.uploadId === undefined ? formatAttachmentMarker(capsule.path, capsule.kind, capsule.label) : capsule.label;
}

/** The text an agent receives: every capsule becomes its tag with an absolute path; an unknown token keeps its label. */
export function serializeCapsuleDraft(
  text: string,
  registry: CapsuleRegistry,
  markerFor: (capsule: ComposerCapsule) => string = capsuleMarker,
): string {
  let out = "";
  let cursor = 0;
  for (const span of capsuleSpans(text)) {
    out += text.slice(cursor, span.start);
    const capsule = registry[span.label];
    out += capsule ? markerFor(capsule) : span.label;
    cursor = span.end;
  }
  return out + text.slice(cursor);
}

export type CapsuleSegment = { text: string } | { span: CapsuleSpan; capsule: ComposerCapsule | null; token: string };

export function capsuleSegments(text: string, registry: CapsuleRegistry): CapsuleSegment[] {
  const segments: CapsuleSegment[] = [];
  let cursor = 0;
  for (const span of capsuleSpans(text)) {
    if (span.start > cursor) segments.push({ text: text.slice(cursor, span.start) });
    segments.push({ span, capsule: registry[span.label] ?? null, token: text.slice(span.start, span.end) });
    cursor = span.end;
  }
  if (cursor < text.length) segments.push({ text: text.slice(cursor) });
  return segments;
}

/** Widens a selection so it never cuts a capsule; a caret inside one snaps to its nearest edge. */
export function expandToCapsules(text: string, start: number, end: number): { start: number; end: number } {
  let from = start;
  let to = end;
  for (const span of capsuleSpans(text)) {
    if (start === end && span.start < start && start < span.end) {
      const edge = start - span.start <= span.end - start ? span.start : span.end;
      return { start: edge, end: edge };
    }
    if (span.start < from && from < span.end) from = span.start;
    if (span.start < to && to < span.end) to = span.end;
  }
  return { start: from, end: to };
}

/** The whole-capsule range Backspace or Delete removes, or null when the native edit is already safe. */
export function capsuleDeletionRange(
  text: string,
  start: number,
  end: number,
  key: "Backspace" | "Delete",
): { start: number; end: number } | null {
  const spans = capsuleSpans(text);
  if (start === end) {
    const hit = spans.find((span) => (key === "Backspace"
      ? span.start < start && start <= span.end
      : span.start <= start && start < span.end));
    return hit ? { start: hit.start, end: hit.end } : null;
  }
  const widened = expandToCapsules(text, start, end);
  return widened.start === start && widened.end === end ? null : widened;
}

/** Where an arrow key lands when the caret sits on a capsule edge; null leaves the move native. */
export function capsuleArrowTarget(text: string, position: number, direction: "left" | "right"): number | null {
  const span = capsuleSpans(text).find((candidate) => (direction === "left"
    ? candidate.start < position && position <= candidate.end
    : candidate.start <= position && position < candidate.end));
  if (!span) return null;
  return direction === "left" ? span.start : span.end;
}

export function encodeCapsuleRegistry(registry: CapsuleRegistry): string {
  const stored = Object.values(registry).filter((capsule) => capsule.uploadId === undefined);
  return stored.length ? JSON.stringify(stored) : "";
}

export function decodeCapsuleRegistry(raw: string): CapsuleRegistry {
  if (!raw) return EMPTY_CAPSULES;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return EMPTY_CAPSULES;
    const registry: Record<string, ComposerCapsule> = {};
    for (const item of parsed) {
      if (!item || typeof item.label !== "string" || typeof item.path !== "string") continue;
      const kind = ATTACHMENT_KINDS.find((candidate) => candidate === item.kind);
      if (kind) registry[item.label] = { label: item.label, kind, path: item.path };
    }
    return registry;
  } catch {
    return EMPTY_CAPSULES;
  }
}
