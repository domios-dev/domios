export interface TaskStridesBlockData {
  taskId?: string;
  count: number;
  body: string;
}

export type TaskStridesSegment =
  | { type: "text"; text: string }
  | { type: "strides"; block: TaskStridesBlockData };

const TAGGED_STRIDES_BLOCK =
  /<codeterm-task-strides((?:\s+[a-z_]+="[^"]*")*)\s*>([\s\S]*?)<\/codeterm-task-strides>/g;
const TAGGED_STRIDES_TEST = /<codeterm-task-strides(?:\s+[a-z_]+="[^"]*")*\s*>/;
const LEGACY_STRIDES_BLOCK =
  /(?:^|\n)(Recent task strides \(read-only (?:context|digest), pinned\/newest first\):\n(?:- \[[^\]]+\][^\n]*(?:\n|$))+(?:Read full strides:[^\n]*(?:\n|$))?)/g;
const LEGACY_STRIDES_TEST =
  /(?:^|\n)Recent task strides \(read-only (?:context|digest), pinned\/newest first\):\n- \[[^\]]+\]/;
const ATTRIBUTE = /([a-z_]+)="([^"]*)"/g;
const STRIDE_ITEM_LINE = /^\s*-\s+\[[^\]]+\]/;

function parseAttributes(source: string): Record<string, string> {
  return Object.fromEntries(Array.from(source.matchAll(ATTRIBUTE), (match) => [match[1], match[2]]));
}

export function countTaskStrideLines(body: string): number {
  let count = 0;
  for (const line of body.replace(/\r\n?/g, "\n").split("\n")) {
    if (STRIDE_ITEM_LINE.test(line)) count += 1;
  }
  return count;
}

export function taskStridesLabel(count: number): string {
  return count === 1 ? "1 task stride" : `${count} task strides`;
}

export function hasTaskStrides(text: string): boolean {
  const normalized = text.replace(/\r\n?/g, "\n");
  return TAGGED_STRIDES_TEST.test(normalized) || LEGACY_STRIDES_TEST.test(normalized);
}

function pushText(segments: TaskStridesSegment[], text: string): void {
  const trimmed = text.replace(/^\n+|\n+$/g, "");
  if (trimmed) segments.push({ type: "text", text: trimmed });
}

function splitLegacyStrides(text: string, segments: TaskStridesSegment[]): boolean {
  let cursor = 0;
  let found = false;
  for (const match of text.matchAll(LEGACY_STRIDES_BLOCK)) {
    const start = match.index + (match[0].startsWith("\n") ? 1 : 0);
    const body = match[1].trim();
    const count = Math.max(1, countTaskStrideLines(body));
    pushText(segments, text.slice(cursor, start));
    segments.push({ type: "strides", block: { count, body } });
    cursor = match.index + match[0].length;
    found = true;
  }
  if (found) pushText(segments, text.slice(cursor));
  return found;
}

export function splitTaskStrides(text: string): TaskStridesSegment[] {
  const normalized = text.replace(/\r\n?/g, "\n");
  if (!hasTaskStrides(normalized)) {
    return normalized ? [{ type: "text", text: normalized }] : [];
  }
  const segments: TaskStridesSegment[] = [];
  let cursor = 0;
  let foundTagged = false;
  for (const match of normalized.matchAll(TAGGED_STRIDES_BLOCK)) {
    const before = normalized.slice(cursor, match.index);
    if (!splitLegacyStrides(before, segments)) pushText(segments, before);
    const attrs = parseAttributes(match[1]);
    const body = match[2].trim();
    const declaredCount = Number(attrs.count);
    const count =
      Number.isInteger(declaredCount) && declaredCount > 0
        ? declaredCount
        : Math.max(1, countTaskStrideLines(body));
    segments.push({
      type: "strides",
      block: {
        ...(attrs.task ? { taskId: attrs.task } : {}),
        count,
        body,
      },
    });
    cursor = match.index + match[0].length;
    foundTagged = true;
  }
  const tail = normalized.slice(cursor);
  if (foundTagged) {
    if (!splitLegacyStrides(tail, segments)) pushText(segments, tail);
    return segments;
  }
  if (splitLegacyStrides(normalized, segments)) {
    return segments;
  }
  return normalized ? [{ type: "text", text: normalized }] : [];
}
