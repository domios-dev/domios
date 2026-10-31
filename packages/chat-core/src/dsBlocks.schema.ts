export const DS_KINDS = ["html", "page", "image", "document", "guide", "plugin", "connect", "setting", "agent", "job", "checklist", "choice", "tasks"] as const;
export type DsKind = typeof DS_KINDS[number];
export const DS_TASK_STATUSES = ["in_progress", "in_review", "blocked", "todo", "pending", "paused", "done", "cancelled"] as const;
export type DsTaskStatus = typeof DS_TASK_STATUSES[number];
export interface DsTaskItem { id: string; key: string; status: DsTaskStatus; name: string }
export interface DsTaskSnapshot { title: string; items: DsTaskItem[] }
export type DsBlock =
  | { kind: "html"; html: string }
  | { kind: "page" | "document"; src: string }
  | { kind: "image"; sources: string[] }
  | { kind: "guide"; section: string }
  | { kind: "plugin" | "checklist"; id: string }
  | { kind: "connect"; bridge: string }
  | { kind: "setting"; key: string }
  | { kind: "agent"; tab: string }
  | { kind: "job"; ref: string }
  | { kind: "choice"; options: string[] }
  | { kind: "tasks"; plan: string; refs: string[]; statuses: DsTaskStatus[]; snapshot?: DsTaskSnapshot };

const ASCII_SPACE = /[\t\n\f\r ]/;
export const isDsKind = (value: string): value is DsKind => (DS_KINDS as readonly string[]).includes(value);
export const isDsTaskRef = (value: string) => /^(?:task_[A-Za-z0-9_-]+|[A-Za-z][A-Za-z0-9]*-\d+)$/.test(value);
export const isDsAbsolutePath = (value: string) => /^(?:[A-Za-z]:[\\/]|\/|\\\\)/.test(value) && !/[\u0000-\u001f\u007f]/.test(value);

export function decodeDsEntities(value: string): string {
  return value.replace(/&(?:amp|lt|gt|quot|apos|#\d+|#x[\da-fA-F]+);/g, (entity) => {
    const named: Record<string, string> = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&apos;": "'" };
    if (named[entity]) return named[entity];
    const hex = entity[2] === "x";
    const code = Number.parseInt(entity.slice(hex ? 3 : 2, -1), hex ? 16 : 10);
    return code > 0 && code <= 0x10ffff && !(code >= 0xd800 && code <= 0xdfff) ? String.fromCodePoint(code) : entity;
  });
}

export interface DsOpening { kind: DsKind; attributes: Record<string, string>; selfClosing: boolean }
export function parseDsOpening(source: string): DsOpening | null {
  let at = 4;
  while (/[a-z]/.test(source[at] ?? "") && at < source.length) at++;
  const kind = source.slice(4, at);
  if (!source.startsWith("<ds-") || !isDsKind(kind)) return null;
  const attributes: Record<string, string> = Object.create(null);
  while (at < source.length) {
    let spaces = 0;
    while (ASCII_SPACE.test(source[at] ?? "") && at < source.length) { at++; spaces++; }
    if (source.slice(at) === ">") return { kind, attributes, selfClosing: false };
    if (source.slice(at) === "/>") return { kind, attributes, selfClosing: true };
    if (!spaces) return null;
    const start = at;
    while (/[a-z]/.test(source[at] ?? "") && at < source.length) at++;
    const name = source.slice(start, at);
    if (!name || Object.prototype.hasOwnProperty.call(attributes, name) || source[at++] !== "=") return null;
    const quote = source[at++];
    if (quote !== '"' && quote !== "'") return null;
    const valueStart = at;
    while (at < source.length && source[at] !== quote) at++;
    if (at === source.length) return null;
    attributes[name] = decodeDsEntities(source.slice(valueStart, at++));
  }
  return null;
}

function stringArray(value: string | undefined): string[] | null {
  if (value === undefined) return [];
  try {
    const parsed: unknown = JSON.parse(value);
    return Array.isArray(parsed) && parsed.every((entry): entry is string => typeof entry === "string") ? parsed : null;
  } catch { return null; }
}

const ALLOWED: Record<DsKind, readonly string[]> = {
  html: [], page: ["src"], document: ["src"], image: ["src", "sources"], guide: ["section"], plugin: ["id"],
  connect: ["bridge"], setting: ["key"], agent: ["tab"], job: ["ref"], checklist: ["id"], choice: ["options"], tasks: ["plan", "refs", "statuses"],
};
export function validateDsBlock(opening: DsOpening, body?: string): DsBlock | null {
  const { kind, attributes: a, selfClosing } = opening;
  if (Object.keys(a).some((key) => !ALLOWED[kind].includes(key))) return null;
  if (kind === "html") return !selfClosing && body !== undefined && new TextEncoder().encode(body).length <= 200_000 ? { kind, html: body } : null;
  if (kind !== "tasks" && (!selfClosing || body !== undefined)) return null;
  const id = a.id ?? "";
  switch (kind) {
    case "page": case "document": return a.src && isDsAbsolutePath(a.src) ? { kind, src: a.src } : null;
    case "image": {
      if (Object.prototype.hasOwnProperty.call(a, "src") === Object.prototype.hasOwnProperty.call(a, "sources")) return null;
      const sources = a.src !== undefined ? [a.src] : stringArray(a.sources);
      return sources && sources.length >= 1 && sources.length <= 32 && sources.every(isDsAbsolutePath) ? { kind, sources } : null;
    }
    case "guide": return /^[a-z0-9][a-z0-9-]*$/.test(a.section ?? "") ? { kind, section: a.section } : null;
    case "plugin": return /^[a-z0-9][a-z0-9._-]{0,63}$/.test(id) && !id.includes("..") ? { kind, id } : null;
    case "checklist": return /^[a-z0-9][a-z0-9-]{0,63}$/.test(id) ? { kind, id } : null;
    case "connect": return /^[a-z][a-z0-9-]{0,31}$/.test(a.bridge ?? "") ? { kind, bridge: a.bridge } : null;
    case "setting": return /^[A-Za-z0-9_][A-Za-z0-9_-]{0,63}(?:\.[A-Za-z0-9_][A-Za-z0-9_-]{0,63}){1,7}$/.test(a.key ?? "") ? { kind, key: a.key } : null;
    case "agent": return /^[A-Za-z0-9][A-Za-z0-9_-]{3,63}$/.test(a.tab ?? "") ? { kind, tab: a.tab } : null;
    case "job": return /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/.test(a.ref ?? "") ? { kind, ref: a.ref } : null;
    case "choice": {
      const options = stringArray(a.options)?.map((label) => label.replace(/\s+/g, " ").trim());
      return options && options.length >= 2 && options.length <= 8 && options.every((label) => label.length > 0 && label.length <= 80)
        && new Set(options.map((label) => label.toLowerCase())).size === options.length ? { kind, options } : null;
    }
    case "tasks": {
      if (!/^pl_[A-Za-z0-9_-]+$/.test(a.plan ?? "")) return null;
      const refs = stringArray(a.refs);
      const statuses = stringArray(a.statuses);
      if (!refs || !refs.every(isDsTaskRef) || !statuses || !statuses.every((status) => (DS_TASK_STATUSES as readonly string[]).includes(status))) return null;
      const block: Extract<DsBlock, { kind: "tasks" }> = { kind, plan: a.plan, refs, statuses: statuses as DsTaskStatus[] };
      if (selfClosing) return body === undefined ? block : null;
      if (body === undefined) return null;
      try {
        const snapshot: unknown = JSON.parse(body);
        if (!snapshot || typeof snapshot !== "object" || Array.isArray(snapshot)) return null;
        const s = snapshot as Record<string, unknown>;
        if (Object.keys(s).some((key) => !["title", "items"].includes(key)) || typeof s.title !== "string" || !Array.isArray(s.items)) return null;
        if (!s.items.every((item: unknown) => {
          if (!item || typeof item !== "object" || Array.isArray(item)) return false;
          const row = item as Record<string, unknown>;
          return Object.keys(row).length === 4 && typeof row.id === "string" && /^task_[A-Za-z0-9_-]+$/.test(row.id)
            && typeof row.key === "string" && /^[A-Za-z][A-Za-z0-9]*-\d+$/.test(row.key)
            && typeof row.name === "string" && typeof row.status === "string" && (DS_TASK_STATUSES as readonly string[]).includes(row.status);
        })) return null;
        return { ...block, snapshot: snapshot as DsTaskSnapshot };
      } catch { return null; }
    }
  }
}

export function serializeDsBlock(block: DsBlock): string {
  const attributes: Record<string, string> = {};
  switch (block.kind) {
    case "html": return `<ds-html>${block.html}</ds-html>`;
    case "page": case "document": attributes.src = block.src; break;
    case "image": if (block.sources.length === 1) attributes.src = block.sources[0]; else attributes.sources = JSON.stringify(block.sources); break;
    case "guide": attributes.section = block.section; break;
    case "plugin": case "checklist": attributes.id = block.id; break;
    case "connect": attributes.bridge = block.bridge; break;
    case "setting": attributes.key = block.key; break;
    case "agent": attributes.tab = block.tab; break;
    case "job": attributes.ref = block.ref; break;
    case "choice": attributes.options = JSON.stringify(block.options); break;
    case "tasks": attributes.plan = block.plan; if (block.refs.length) attributes.refs = JSON.stringify(block.refs); if (block.statuses.length) attributes.statuses = JSON.stringify(block.statuses); break;
  }
  const escape = (value: string) => value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const opening = `<ds-${block.kind}${Object.entries(attributes).map(([name, value]) => ` ${name}="${escape(value)}"`).join("")}`;
  return block.kind === "tasks" && block.snapshot ? `${opening}>${JSON.stringify(block.snapshot)}</ds-tasks>` : `${opening} />`;
}
