export type ReferenceKind = "plan" | "task" | "section" | "tab";
export interface ReferenceMarker { start: number; end: number; text: string; kind: ReferenceKind; id: string; plan?: string }
const MARKER = /<domios\s+([^<>]*?)\s*\/>/g;
const ATTRIBUTE = /([a-z]+)="([^"<>]*)"/g;
const decode = (value: string) => value.replace(/&quot;/g, '"').replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
const encode = (value: string) => value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

export function referenceMarkerText(kind: ReferenceKind, id: string, plan?: string): string {
  return `<domios type="${kind}" id="${encode(id)}"${plan ? ` plan="${encode(plan)}"` : ""} />`;
}

export function referenceMarkers(text: string): ReferenceMarker[] {
  if (!text.includes("<domios")) return [];
  const rows: ReferenceMarker[] = [];
  // Code is syntax, even when it contains a complete marker example.
  const tokens = /(`{3,}|~{3,})[^\n]*\n[\s\S]*?(?:\n\1[^\n]*(?:\n|$)|$)|(`+)[\s\S]*?\2|<domios\s+[^<>]*?\s*\/>/g;
  for (const token of text.matchAll(tokens)) {
    if (!token[0].startsWith("<domios")) continue;
    const match = [...token[0].matchAll(MARKER)][0];
    if (!match) continue;
    const attrs = new Map<string, string>();
    let cursor = 0;
    let valid = true;
    for (const attr of match[1].matchAll(ATTRIBUTE)) {
      if (match[1].slice(cursor, attr.index).trim() || attrs.has(attr[1])) { valid = false; break; }
      attrs.set(attr[1], decode(attr[2]));
      cursor = attr.index! + attr[0].length;
    }
    if (!valid || match[1].slice(cursor).trim()) continue;
    const kind = attrs.get("type");
    const id = attrs.get("id");
    const plan = attrs.get("plan");
    if (!id || !["plan", "task", "section", "tab"].includes(kind ?? "")) continue;
    if (kind === "section" && !plan) continue;
    rows.push({ start: token.index!, end: token.index! + token[0].length, text: token[0], kind: kind as ReferenceKind, id, plan });
  }
  return rows;
}
