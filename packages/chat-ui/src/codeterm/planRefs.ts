export interface TextSegment {
  kind: "text";
  value: string;
}
export interface PlanRefLinkSegment {
  kind: "plan_ref";
  value: string;
  planId: string;
  targetId?: string;
  targetKind: "plan" | "task" | "section";
}
export type PlanRefSegment = TextSegment | PlanRefLinkSegment;

const REF_G = /\b(?:pl|task|sec)_[a-z0-9]{1,32}\b/g;
const PLAN_RE = /\bpl_[a-z0-9]{1,32}\b/;
const URL_RE = /\b(?:https?:\/\/|www\.)\S+\b/g;
const FENCE_RE = /```[\s\S]*?(?:```|$)/g;
const INLINE_CODE_RE = /`[^`\n]+`/g;
const MD_LINK_RE = /\[[^\]]*]\([^)]+\)/g;

function targetKind(value: string): PlanRefLinkSegment["targetKind"] {
  if (value.startsWith("task_")) return "task";
  if (value.startsWith("sec_")) return "section";
  return "plan";
}

function isCommandLine(line: string): boolean {
  const trimmed = line.trimStart();
  return trimmed.startsWith("$ ")
    || trimmed.startsWith("> ")
    || trimmed.startsWith("domios ")
    || trimmed.startsWith("codeterm ")
    || trimmed.startsWith("./build.sh ")
    || trimmed.startsWith("npx ")
    || trimmed.startsWith("npm ")
    || trimmed.startsWith("pnpm ")
    || trimmed.startsWith("cargo ")
    || trimmed.startsWith("git ")
    || trimmed.startsWith("curl ")
    || trimmed.startsWith("wget ")
    || trimmed.startsWith("node ")
    || trimmed.startsWith("tsx ")
    || trimmed.startsWith("python ")
    || trimmed.startsWith("pwsh ")
    || trimmed.startsWith("powershell ")
    || trimmed.startsWith("bash ")
    || trimmed.startsWith("sh ");
}

/** Sorted, disjoint [start, end) spans that never link. */
type Mask = Array<[number, number]>;

function buildMask(text: string): Mask {
  const spans: Mask = [];
  for (const regex of [FENCE_RE, INLINE_CODE_RE, MD_LINK_RE, URL_RE]) {
    regex.lastIndex = 0;
    for (const match of text.matchAll(regex)) {
      const start = match.index ?? 0;
      if (match[0].length > 0) spans.push([start, start + match[0].length]);
    }
  }
  for (let lineStart = 0; lineStart <= text.length;) {
    const newline = text.indexOf("\n", lineStart);
    const lineEnd = newline < 0 ? text.length : newline;
    if (lineEnd > lineStart && isCommandLine(text.slice(lineStart, lineEnd))) spans.push([lineStart, lineEnd]);
    lineStart = lineEnd + 1;
  }
  spans.sort((a, b) => a[0] - b[0]);
  const merged: Mask = [];
  for (const span of spans) {
    const last = merged[merged.length - 1];
    if (last && span[0] <= last[1]) last[1] = Math.max(last[1], span[1]);
    else merged.push([span[0], span[1]]);
  }
  return merged;
}

function isMasked(masked: Mask, start: number, end: number): boolean {
  let low = 0;
  let high = masked.length;
  while (low < high) {
    const mid = (low + high) >> 1;
    if (masked[mid][1] <= start) low = mid + 1;
    else high = mid;
  }
  return low < masked.length && masked[low][0] < end;
}

function maskText(text: string, masked: Mask): string {
  let result = "";
  let cursor = 0;
  for (const [start, end] of masked) {
    result += text.slice(cursor, start) + " ".repeat(end - start);
    cursor = end;
  }
  return result + text.slice(cursor);
}

function safeMatch(text: string, masked: Mask, from: number): RegExpMatchArray | null {
  REF_G.lastIndex = from;
  let match: RegExpMatchArray | null;
  while ((match = REF_G.exec(text)) != null) {
    const start = match.index ?? 0;
    if (!isMasked(masked, start, start + match[0].length)) return match;
  }
  return null;
}

function lineStartAt(text: string, index: number): number {
  return index <= 0 ? 0 : text.lastIndexOf("\n", index - 1) + 1;
}

function nearestPlanIdOnLine(safeText: string, masked: Mask, matchStart: number): string | undefined {
  const lineStart = lineStartAt(safeText, matchStart);
  const line = safeText.slice(lineStart, matchStart);
  PLAN_RE.lastIndex = 0;
  let found: string | undefined;
  for (const match of line.matchAll(new RegExp(PLAN_RE.source, "g"))) {
    const absoluteStart = lineStart + (match.index ?? 0);
    if (!isMasked(masked, absoluteStart, absoluteStart + match[0].length)) found = match[0];
  }
  return found;
}

function nextPlanIdOnLine(safeText: string, masked: Mask, matchEnd: number): string | undefined {
  const lineEnd = safeText.indexOf("\n", matchEnd);
  const absoluteLineEnd = lineEnd === -1 ? safeText.length : lineEnd;
  const line = safeText.slice(matchEnd, absoluteLineEnd);
  for (const match of line.matchAll(new RegExp(PLAN_RE.source, "g"))) {
    const absoluteStart = matchEnd + (match.index ?? 0);
    const absoluteEnd = absoluteStart + match[0].length;
    const bridge = safeText.slice(matchEnd, absoluteStart);
    if (!/\b(?:in|under|inside|within|for|of)\s*$/i.test(bridge)) continue;
    if (!isMasked(masked, absoluteStart, absoluteEnd)) return match[0];
  }
  return undefined;
}

/** Split text into plain + linkable plan/task/section reference segments. */
export function linkifyPlanRefs(text: string): PlanRefSegment[] {
  if (!text) return [{ kind: "text", value: text ?? "" }];
  REF_G.lastIndex = 0;
  if (!REF_G.test(text)) return [{ kind: "text", value: text }];
  const masked = buildMask(text);
  const safeText = maskText(text, masked);

  const segments: PlanRefSegment[] = [];
  let last = 0;
  let from = 0;
  while (true) {
    const match = safeMatch(text, masked, from);
    if (!match) break;
    const value = match[0];
    const kind = targetKind(value);
    const start = match.index ?? 0;
    const end = start + value.length;
    const planId = kind === "plan"
      ? value
      : nearestPlanIdOnLine(safeText, masked, start) ?? nextPlanIdOnLine(safeText, masked, end);
    if (!planId) {
      from = end;
      continue;
    }
    if (start > last) segments.push({ kind: "text", value: text.slice(last, start) });
    segments.push({
      kind: "plan_ref",
      value,
      planId,
      targetId: kind === "plan" ? undefined : value,
      targetKind: kind,
    });
    last = start + value.length;
    from = last;
  }
  if (segments.length === 0) return [{ kind: "text", value: text }];
  if (last < text.length) segments.push({ kind: "text", value: text.slice(last) });
  return segments;
}

export function extractPlanRefs(text: string): PlanRefLinkSegment[] {
  const seen = new Set<string>();
  const refs: PlanRefLinkSegment[] = [];
  for (const seg of linkifyPlanRefs(text)) {
    if (seg.kind !== "plan_ref") continue;
    const key = `${seg.targetKind}:${seg.planId}:${seg.targetId ?? ""}`;
    if (seen.has(key)) continue;
    seen.add(key);
    refs.push(seg);
  }
  return refs;
}

export function hasPlanRefs(text: string): boolean {
  return linkifyPlanRefs(text).some((s) => s.kind === "plan_ref");
}
