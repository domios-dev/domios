import { defineProviderIo, ioValue, type Host, type HostIoSteps } from "@codeterm/plugin-sdk";
type MemHit = { id?: unknown; text?: unknown; score?: unknown };
type PresetKind = "mem-recall" | "template";

type UserPreset = {
  id?: unknown;
  name?: unknown;
  kind?: unknown;
  template?: unknown;
  config?: unknown;
};

type Settings = {
  enabled?: unknown;
  activePreset?: unknown;
  memK?: unknown;
  memMaxChars?: unknown;
  insertPosition?: unknown;
  presets?: unknown;
};

type HostShape = {
  settingsJson?: () => string;

};

declare const host: Host;

const SYSTEM_MEM_RECALL_ID = "mem-recall";

function currentHost(): HostShape {
  return typeof host === "object" && host ? host : {};
}

function readSettings(): Settings {
  const raw = currentHost().settingsJson?.();
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed as Settings : {};
  } catch {
    return {};
  }
}

function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function asNumber(value: unknown, fallback: number, min: number, max: number): number {
  const n = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(min, Math.min(max, Math.floor(n)));
}

function asPosition(value: unknown): "prepend" | "append" {
  return value === "append" ? "append" : "prepend";
}

function compactSnippet(text: string, maxChars: number): string {
  const oneLine = text.replace(/\s+/g, " ").trim();
  if (oneLine.length <= maxChars) return oneLine;
  return `${oneLine.slice(0, Math.max(0, maxChars - 1)).trimEnd()}…`;
}

function formatMemBlock(text: string, settings: Settings, overrides?: Record<string, unknown>, hits: MemHit[] = []): string {
  const k = asNumber(overrides?.k ?? settings.memK, 3, 1, 8);
  const maxChars = asNumber(overrides?.maxChars ?? settings.memMaxChars, 140, 40, 500);
  const rows = hits
    .filter((hit) => typeof hit.id === "string" && typeof hit.text === "string")
    .slice(0, k)
    .map((hit) => `- ${hit.id}: ${compactSnippet(String(hit.text), maxChars)}`);
  if (rows.length === 0) return "";
  return `Found similar in mem:\n${rows.join("\n")}`;
}

function applyPosition(text: string, block: string, position: "prepend" | "append"): string {
  if (!block.trim()) return text;
  return position === "append" ? `${text}\n\n${block}` : `${block}\n\n${text}`;
}

function userPresets(settings: Settings): UserPreset[] {
  return Array.isArray(settings.presets) ? settings.presets.filter((p) => p && typeof p === "object") as UserPreset[] : [];
}

function findPreset(settings: Settings): { kind: PresetKind; preset?: UserPreset; config: Record<string, unknown> } | null {
  const active = asString(settings.activePreset, SYSTEM_MEM_RECALL_ID).trim() || SYSTEM_MEM_RECALL_ID;
  if (active === SYSTEM_MEM_RECALL_ID) return { kind: "mem-recall", config: {} };
  const preset = userPresets(settings).find((p) => asString(p.id).trim() === active);
  if (!preset) return null;
  const kind = asString(preset.kind, "template") === "mem-recall" ? "mem-recall" : "template";
  const config = preset.config && typeof preset.config === "object" && !Array.isArray(preset.config)
    ? preset.config as Record<string, unknown>
    : {};
  return { kind, preset, config };
}

function renderTemplate(template: string, text: string, memBlock: string): string {
  return template.replaceAll("{{text}}", text).replaceAll("{{mem}}", memBlock);
}

export function preprocessText(text: string, settings: Settings, hits: MemHit[] = []): string | null {
  if (settings.enabled !== true) return null;
  const selected = findPreset(settings);
  if (!selected) return null;
  if (selected.kind === "mem-recall") {
    const block = formatMemBlock(text, settings, selected.config, hits);
    return applyPosition(text, block, asPosition(selected.config.position ?? settings.insertPosition));
  }
  const template = asString(selected.preset?.template, "{{text}}");
  const memBlock = template.includes("{{mem}}") ? formatMemBlock(text, settings, selected.config, hits) : "";
  return renderTemplate(template, text, memBlock);
}

const plugin = {
  meta: {
    id: "chat-preprocessor",
    displayName: "Chat Preprocessor",
  },

  match: "text",

  *chatPreprocess(ctx: { text?: unknown }): HostIoSteps<{text: string} | null> {
    const text = asString(ctx?.text);
    if (!text.trim()) return null;
    const settings = readSettings();
    const selected = settings.enabled === true ? findPreset(settings) : null;
    const needsMem = selected && (selected.kind === "mem-recall" || asString(selected.preset?.template).includes("{{mem}}"));
    let hits: MemHit[] = [];
    if (needsMem) {
      const k = asNumber(selected.config.k ?? settings.memK, 3, 1, 8);
      try { hits = (yield* ioValue<{hits?: MemHit[]}>({op: "memSearch", options: {query: text, k}}))?.hits ?? []; }
      catch { return null; }
    }
    const next = preprocessText(text, settings, hits);
    return next === null || next === text ? null : { text: next };
  },
};

export default defineProviderIo(plugin, () => host.io);
