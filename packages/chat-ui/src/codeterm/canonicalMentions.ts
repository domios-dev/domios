import { DS_TASK_KEY_RE } from "./dsTasksBlock";
export type CanonicalMentionKind = "tab" | "plan" | "blueprint" | "task";
export interface CanonicalMention { start: number; end: number; text: string; label: string; kind: CanonicalMentionKind; id: string }
export function canonicalMentionText(kind: Exclude<CanonicalMentionKind, "task">, title: string, id: string): string {
  const label = title.replace(/[@()`]/g, "").trim().replace(/\s+/g, "_").slice(0, 80) || kind;
  return `@${label} (${kind} ${id})`;
}
export function canonicalMentions(text: string): CanonicalMention[] {
  const masked: Array<[number, number]> = [];
  for (const match of text.matchAll(/```[\s\S]*?(?:```|$)|`[^`\n]+`|\[[^\]]*\]\([^)]+\)|https?:\/\/\S+/g)) masked.push([match.index!, match.index! + match[0].length]);
  const rows: CanonicalMention[] = [];
  for (const match of text.matchAll(/(^|\s)@([^\s@()]{1,80}) \((tab|plan|blueprint) ([a-zA-Z0-9_:.\/-]+)\)|\b[A-Za-z][A-Za-z0-9]*-\d+\b/g)) {
    const canonical = match[3] !== undefined;
    if (!canonical && !DS_TASK_KEY_RE.test(match[0])) continue;
    const start = match.index! + (canonical ? match[1].length : 0);
    const end = match.index! + match[0].length;
    if (masked.some(([a, b]) => start < b && end > a)) continue;
    rows.push({ start, end, text: text.slice(start, end), label: match[2] ?? match[0], kind: (match[3] ?? "task") as CanonicalMentionKind, id: match[4] ?? match[0] });
  }
  return rows;
}
export function hasCanonicalMentions(text: string): boolean { return canonicalMentions(text).length > 0; }
