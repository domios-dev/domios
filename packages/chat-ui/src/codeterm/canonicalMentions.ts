export interface CanonicalMention { start: number; end: number; text: string; label: string; kind: "tab" | "plan" | "blueprint" | "task"; id: string }
export function canonicalMentions(text: string): CanonicalMention[] {
  const masked: Array<[number, number]> = [];
  for (const match of text.matchAll(/```[\s\S]*?(?:```|$)|`[^`\n]+`|\[[^\]]*\]\([^)]+\)|https?:\/\/\S+/g)) masked.push([match.index!, match.index! + match[0].length]);
  const rows: CanonicalMention[] = [];
  for (const match of text.matchAll(/@([^\n@]+?) \((tab|plan|blueprint) ([a-zA-Z0-9_:.\/-]+)\)|\bCOD-\d+\b/g)) {
    const start = match.index!;
    const end = start + match[0].length;
    if (masked.some(([a, b]) => start < b && end > a)) continue;
    rows.push({ start, end, text: match[0], label: match[1] ?? match[0], kind: (match[2] ?? "task") as CanonicalMention["kind"], id: match[3] ?? match[0] });
  }
  return rows;
}
export function hasCanonicalMentions(text: string): boolean { return canonicalMentions(text).length > 0; }
