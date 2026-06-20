// Model discovery for the Claude provider. API-key users get the live
// /v1/models list mapped ONTO the curated aliases (opus/sonnet/haiku/fable):
// each alias keeps its stable id + curated description but takes its version
// label and availability from the matching live model. OAuth accounts have no
// cheap model-list probe → null → caller falls back to the curated manifest.
import type { ModelInfo } from "@codeterm/plugin-sdk";
import { manifest } from "./shared";

interface AnthropicModelsResponse { data?: AnthropicModel[] }
interface AnthropicModel { id?: string; display_name?: string; displayName?: string }

const FAMILIES = ["opus", "sonnet", "haiku", "fable"];

export function discoverAnthropicModels(): ModelInfo[] | null {
  const key = host.envGet("ANTHROPIC_API_KEY") || host.envGet("CLAUDE_API_KEY");
  if (!key) return null;
  const live = fetchLiveModels(key);
  if (!live) return null;
  const mapped = mapAliasesToLive(manifestModels(), live);
  return mapped.length ? mapped : null;
}

export function manifestModels(): ModelInfo[] {
  const m = manifest();
  return Array.isArray(m && m.models) ? m!.models! : [];
}

function fetchLiveModels(key: string): AnthropicModel[] | null {
  const raw = host.fetch(JSON.stringify({
    url: "https://api.anthropic.com/v1/models",
    method: "GET",
    timeoutMs: 3000,
    headers: {
      "x-api-key": key,
      "anthropic-version": "2023-06-01",
    },
  }));
  const response = safeJson(raw) as { status?: number; body?: string } | null;
  if (!response || response.status !== 200 || typeof response.body !== "string") return null;
  const body = safeJson(response.body) as AnthropicModelsResponse | null;
  const data = body && body.data;
  if (!Array.isArray(data)) return null;
  const out: AnthropicModel[] = [];
  for (let i = 0; i < data.length; i++) {
    const item = data[i];
    if (item && typeof item.id === "string" && item.id.trim()) out.push(item);
  }
  return out.length ? out : null;
}

// For each curated alias, find its matching live model: present → version label
// from live + available; absent → keep the curated label and mark unavailable
// (e.g. fable until the account can use it). The alias SET stays curated.
function mapAliasesToLive(aliases: ModelInfo[], live: AnthropicModel[]): ModelInfo[] {
  const out: ModelInfo[] = [];
  for (let i = 0; i < aliases.length; i++) {
    const alias = aliases[i];
    const family = familyOf(alias);
    const match = family ? bestLiveMatch(live, family) : null;
    const mapped: ModelInfo = {
      id: alias.id,
      displayName: match ? withVersion(alias.displayName, versionOf(match)) : alias.displayName,
    };
    if (alias.description) mapped.description = alias.description;
    mapped.available = !!match;
    out.push(mapped);
  }
  return out;
}

function familyOf(alias: ModelInfo): string | null {
  const hay = (alias.id + " " + alias.displayName).toLowerCase();
  for (let i = 0; i < FAMILIES.length; i++) {
    if (hay.indexOf(FAMILIES[i]) !== -1) return FAMILIES[i];
  }
  return null;
}

function bestLiveMatch(live: AnthropicModel[], family: string): AnthropicModel | null {
  let best: AnthropicModel | null = null;
  let bestKey = -1;
  for (let i = 0; i < live.length; i++) {
    const m = live[i];
    const hay = (String(m.id || "") + " " + liveDisplay(m)).toLowerCase();
    if (hay.indexOf(family) === -1) continue;
    const k = versionKey(versionOf(m));
    if (k >= bestKey) { bestKey = k; best = m; }
  }
  return best;
}

function liveDisplay(m: AnthropicModel): string {
  if (typeof m.display_name === "string" && m.display_name.trim()) return m.display_name.trim();
  if (typeof m.displayName === "string" && m.displayName.trim()) return m.displayName.trim();
  return "";
}

// "4.8" / "5" — prefer the display name, fall back to the id whose version
// segments are dash-separated (claude-opus-4-8 → 4.8).
function versionOf(m: AnthropicModel): string {
  const fromName = matchVersion(liveDisplay(m));
  if (fromName) return fromName;
  const id = String(m.id || "");
  const seg = /(\d+)-(\d+)/.exec(id);
  if (seg) return seg[1] + "." + seg[2];
  return matchVersion(id.replace(/-/g, "."));
}

function matchVersion(s: string): string {
  const m = /(\d+(?:\.\d+)?)/.exec(s);
  return m ? m[1] : "";
}

function versionKey(v: string): number {
  if (!v) return 0;
  const parts = v.split(".");
  const major = parseInt(parts[0], 10) || 0;
  const minor = parts.length > 1 ? (parseInt(parts[1], 10) || 0) : 0;
  return major * 1000 + minor;
}

// Substitute the live version into the curated label, preserving the family
// word and any suffix like "(1M context)"; append if there's no version token.
function withVersion(displayName: string, version: string): string {
  if (!version) return displayName;
  if (/\d+(?:\.\d+)?/.test(displayName)) return displayName.replace(/\d+(?:\.\d+)?/, version);
  return displayName + " " + version;
}

function safeJson(s: string | null | undefined): unknown {
  try { return JSON.parse(String(s || "null")); } catch (_) { return null; }
}
