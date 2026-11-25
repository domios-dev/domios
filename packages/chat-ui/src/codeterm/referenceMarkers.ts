export type ReferenceKind = "plan" | "task" | "section" | "tab";
export interface ReferenceMarker { start: number; end: number; text: string; kind: ReferenceKind; id: string; plan?: string; peer?: string }
const MARKER = /<domios\s+([^<>]*?)\s*\/>/g;
const ATTRIBUTE = /([a-z]+)="([^"<>]*)"/g;
const decode = (value: string) => value.replace(/&quot;/g, '"').replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
const encode = (value: string) => value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

const ALLOWED_ATTRS: Record<ReferenceKind, ReadonlySet<string>> = {
  plan: new Set(["type", "id"]),
  task: new Set(["type", "id", "plan"]),
  section: new Set(["type", "id", "plan"]),
  tab: new Set(["type", "id", "peer"]),
};

export function referenceMarkerText(kind: ReferenceKind, id: string, plan?: string, peer?: string): string {
  const extra = kind === "tab"
    ? (peer ? ` peer="${encode(peer)}"` : "")
    : (plan ? ` plan="${encode(plan)}"` : "");
  return `<domios type="${kind}" id="${encode(id)}"${extra} />`;
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
    const peer = attrs.get("peer");
    if (!id || !["plan", "task", "section", "tab"].includes(kind ?? "")) continue;
    const allowed = ALLOWED_ATTRS[kind as ReferenceKind];
    if ([...attrs.keys()].some((name) => !allowed.has(name))) continue;
    if (kind === "section" && !plan) continue;
    if (peer !== undefined && (!peer || /[/?&#\s]/.test(peer) || id.startsWith("mesh:"))) continue;
    rows.push({ start: token.index!, end: token.index! + token[0].length, text: token[0], kind: kind as ReferenceKind, id, ...(plan ? { plan } : {}), ...(peer ? { peer } : {}) });
  }
  return rows;
}
