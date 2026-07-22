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

function maskRanges(text: string, regex: RegExp, masked: boolean[]): void {
  regex.lastIndex = 0;
  for (const match of text.matchAll(regex)) {
    const start = match.index ?? 0;
    const end = start + match[0].length;
    for (let i = start; i < end; i += 1) masked[i] = true;
  }
}

function buildMask(text: string): boolean[] {
  const masked = Array.from({ length: text.length }, () => false);
  maskRanges(text, FENCE_RE, masked);
  maskRanges(text, INLINE_CODE_RE, masked);
  maskRanges(text, MD_LINK_RE, masked);
  maskRanges(text, URL_RE, masked);
  let lineStart = 0;
  for (let i = 0; i <= text.length; i += 1) {
    if (i !== text.length && text[i] !== "\n") continue;
    const line = text.slice(lineStart, i);
    if (isCommandLine(line)) {
      for (let j = lineStart; j < i; j += 1) masked[j] = true;
    }
    lineStart = i + 1;
  }
  return masked;
}

function safeMatch(text: string, masked: boolean[], from: number): RegExpMatchArray | null {
  REF_G.lastIndex = from;
  let match: RegExpMatchArray | null;
  while ((match = REF_G.exec(text)) != null) {
    const start = match.index ?? 0;
    const end = start + match[0].length;
    let blocked = false;
    for (let i = start; i < end; i += 1) {
      if (masked[i]) {
        blocked = true;
        break;
      }
    }
    if (!blocked) return match;
  }
  return null;
}

function lineStartAt(text: string, index: number): number {
  for (let i = index; i >= 0; i -= 1) {
    if (i === 0 || text[i - 1] === "\n") return i;
  }
  return 0;
}

function nearestPlanIdOnLine(safeText: string, masked: boolean[], matchStart: number): string | undefined {
  const lineStart = lineStartAt(safeText, matchStart);
  const line = safeText.slice(lineStart, matchStart);
  PLAN_RE.lastIndex = 0;
  let found: string | undefined;
  for (const match of line.matchAll(new RegExp(PLAN_RE.source, "g"))) {
    const absoluteStart = lineStart + (match.index ?? 0);
    const absoluteEnd = absoluteStart + match[0].length;
    let blocked = false;
    for (let i = absoluteStart; i < absoluteEnd; i += 1) {
      if (masked[i]) {
        blocked = true;
        break;
      }
    }
    if (!blocked) found = match[0];
  }
  return found;
}

function nextPlanIdOnLine(safeText: string, masked: boolean[], matchEnd: number): string | undefined {
  const lineEnd = safeText.indexOf("\n", matchEnd);
  const absoluteLineEnd = lineEnd === -1 ? safeText.length : lineEnd;
  const line = safeText.slice(matchEnd, absoluteLineEnd);
  for (const match of line.matchAll(new RegExp(PLAN_RE.source, "g"))) {
    const absoluteStart = matchEnd + (match.index ?? 0);
    const absoluteEnd = absoluteStart + match[0].length;
    const bridge = safeText.slice(matchEnd, absoluteStart);
    if (!/\b(?:in|under|inside|within|for|of)\s*$/i.test(bridge)) continue;
    let blocked = false;
    for (let i = absoluteStart; i < absoluteEnd; i += 1) {
      if (masked[i]) {
        blocked = true;
        break;
      }
    }
    if (!blocked) return match[0];
  }
  return undefined;
}

/** Split text into plain + linkable plan/task/section reference segments. */
export function linkifyPlanRefs(text: string): PlanRefSegment[] {
  if (!text) return [{ kind: "text", value: text ?? "" }];
  const masked = buildMask(text);
  const safeText = text
    .split("")
    .map((ch, i) => (masked[i] ? " " : ch))
    .join("");

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
