import { referenceMarkers, referenceMarkerText } from "./referenceMarkers";
export type CanonicalMentionKind = "tab" | "plan" | "blueprint" | "task";
export interface CanonicalMention { start: number; end: number; text: string; label: string; kind: CanonicalMentionKind; id: string }
export function canonicalMentionText(kind: Exclude<CanonicalMentionKind, "task">, title: string, id: string): string {
  if (kind !== "blueprint") return referenceMarkerText(kind, id);
  const label = title.replace(/[@()`]/g, "").trim().replace(/\s+/g, "_").slice(0, 80) || kind;
  return `@${label} (${kind} ${id})`;
}
export function canonicalMentions(text: string): CanonicalMention[] {
  const rows: CanonicalMention[] = [];
  for (const marker of referenceMarkers(text)) {
    if (marker.kind !== "tab" && !(marker.kind === "task" && !marker.plan)) continue;
    rows.push({ ...marker, label: marker.id, kind: marker.kind });
  }
  if (!text.includes("(blueprint ")) return rows;
  const masked: Array<[number, number]> = [];
  for (const match of text.matchAll(/```[\s\S]*?(?:```|$)|`[^`\n]+`|\[[^\]]*\]\([^)]+\)|https?:\/\/\S+/g)) masked.push([match.index!, match.index! + match[0].length]);
  for (const match of text.matchAll(/(^|\s)@([^\s@()]{1,80}) \(blueprint ([a-zA-Z0-9_:.\/-]+)\)/g)) {
    const start = match.index! + match[1].length;
    const end = match.index! + match[0].length;
    if (masked.some(([a, b]) => start < b && end > a)) continue;
    rows.push({ start, end, text: text.slice(start, end), label: match[2], kind: "blueprint", id: match[3] });
  }
  rows.sort((a, b) => a.start - b.start);
  return rows;
}
export function hasCanonicalMentions(text: string): boolean { return canonicalMentions(text).length > 0; }
