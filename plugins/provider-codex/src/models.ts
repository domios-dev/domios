import type { ModelInfo } from "@codeterm/plugin-sdk";

declare const host: {
  envGet(name: string): string | null;
  homeDir(): string | null;
  platform(): string;
  fs: {
    fileExists(path: string): boolean;
    readJson<T>(path: string): T | null;
  };
};

interface CodexModelCache {
  models?: CodexCachedModel[];
}

interface CodexCachedModel {
  slug?: string;
  display_name?: string;
  description?: string;
  visibility?: string;
  priority?: number;
  supported_reasoning_levels?: CodexReasoningLevel[];
  default_reasoning_level?: string;
}

interface CodexReasoningLevel {
  effort?: string;
  description?: string;
}

// `null` tells the host no genuine discovery ran; the seed is the host's to apply.
export function discoverCodexModels(): ModelInfo[] | null {
  return discoverCodexModelsFromCache();
}

function codexModelsCachePath(): string | null {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "models_cache.json";
}

function discoverCodexModelsFromCache(): ModelInfo[] | null {
  const path = codexModelsCachePath();
  if (!path || !host.fs.fileExists(path)) return null;
  const cache = host.fs.readJson<CodexModelCache>(path);
  const raw = cache && cache.models;
  if (!Array.isArray(raw)) return null;
  const kept: CodexCachedModel[] = [];
  for (let i = 0; i < raw.length; i++) {
    const item = raw[i];
    const id = typeof item.slug === "string" ? item.slug.trim() : "";
    if (!id) continue;
    if (item.visibility === "hide") continue;
    if (isNonChatModel(id)) continue;
    kept.push(item);
  }
  kept.sort((a, b) => priorityOf(a) - priorityOf(b));
  const models: ModelInfo[] = [];
  for (let i = 0; i < kept.length; i++) {
    const item = kept[i];
    const id = (item.slug as string).trim();
    models.push({
      id,
      displayName: typeof item.display_name === "string" && item.display_name.trim()
        ? item.display_name.trim()
        : displayNameFromId(id),
      description: typeof item.description === "string" && item.description.trim()
        ? item.description.trim()
        : undefined,
      reasoningEfforts: reasoningEffortsFrom(item.supported_reasoning_levels),
      defaultReasoningEffort: defaultEffortFrom(item),
    });
  }
  return models.length ? models : null;
}

function defaultEffortFrom(model: CodexCachedModel): string | undefined {
  const effort = typeof model.default_reasoning_level === "string"
    ? model.default_reasoning_level.trim()
    : "";
  return effort || undefined;
}

function reasoningEffortsFrom(levels: CodexReasoningLevel[] | undefined): ModelInfo["reasoningEfforts"] {
  if (!Array.isArray(levels)) return undefined;
  const efforts: NonNullable<ModelInfo["reasoningEfforts"]> = [];
  const seen = new Set<string>();
  for (const level of levels) {
    const id = typeof level.effort === "string" ? level.effort.trim() : "";
    if (!id || seen.has(id)) continue;
    seen.add(id);
    efforts.push({
      id,
      displayName: id,
      description: typeof level.description === "string" && level.description.trim()
        ? level.description.trim()
        : undefined,
    });
  }
  return efforts.length ? efforts : undefined;
}

function priorityOf(m: CodexCachedModel): number {
  return typeof m.priority === "number" ? m.priority : 1e9;
}

const NON_CHAT_TOKENS = ["embed", "embedding", "embeddings", "tts", "whisper", "moderation", "rerank", "dall", "dalle"];

function isNonChatModel(id: string): boolean {
  const leaf = (id.split("/").pop() || id).toLowerCase();
  const toks = leaf.split(/[-_.]/);
  for (let i = 0; i < toks.length; i++) {
    if (NON_CHAT_TOKENS.indexOf(toks[i]) !== -1) return true;
  }
  return false;
}

function displayNameFromId(id: string): string {
  const parts = id.split("/");
  const leaf = parts[parts.length - 1] || id;
  return leaf.split("-").filter(Boolean).map(capitalizeToken).join(" ");
}

function capitalizeToken(token: string): string {
  return token ? token.charAt(0).toUpperCase() + token.substring(1) : token;
}
