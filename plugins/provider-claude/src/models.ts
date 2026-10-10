// /v1/models list mapped onto the aliases accepted by the installed CLI.
import type { ModelInfo } from "@codeterm/plugin-sdk";
import { manifest } from "./shared";
import {modelWithCapabilities} from "./modelCapabilities";

interface AnthropicModelsResponse { data?: AnthropicModel[] }
interface AnthropicModel { id?: string; display_name?: string; displayName?: string }

const FAMILIES = ["opus", "sonnet", "haiku", "fable"];

/** What the `claude --version` probe came back with. */
export type CliProbe =
  | { kind: "version"; version: number[] }
  | { kind: "unreachable" };

export type Availability = "available" | "unavailable" | "unknown";

/**
 * The whole decision, pure and I/O-free. A probe that could not run reports
 * unknown: unavailable means the CLI told us it cannot, never that we failed
 * to ask. Callers that already hold a definite negative pass it as `declared`.
 */
export function decideAvailability(
  probe: CliProbe,
  minimum: number[],
  declared?: boolean,
): Availability {
  if (declared === false) return "unavailable";
  if (probe.kind !== "version") return "unknown";
  return atLeast(probe.version, minimum) ? "available" : "unavailable";
}

export function discoverAnthropicModels(): ModelInfo[] | null {
  const installed = probeInstalledCli();
  const key = host.envGet("ANTHROPIC_API_KEY") || host.envGet("CLAUDE_API_KEY");
  if (!key) return applyInstalledCapabilities(manifestModels(), installed);
  const live = fetchLiveModels(key);
  if (!live) return applyInstalledCapabilities(manifestModels(), installed);
  const mapped = mapAliasesToLive(manifestModels(), live);
  return mapped.length ? applyInstalledCapabilities(mapped, installed, true) : null;
}

export function manifestModels(): ModelInfo[] {
  const m = manifest();
  if (!Array.isArray(m && m.models)) return [];
  return m!.models!.map((model) => modelWithCapabilities({
    ...model,
    id: model.id,
    displayName: model.displayName,
    ...(model.description ? { description: model.description } : {}),
  }));
}

function fetchLiveModels(key: string): AnthropicModel[] | null {
  let raw: string;
  try {
    raw = host.fetch(JSON.stringify({
      url: "https://api.anthropic.com/v1/models",
      method: "GET",
      timeoutMs: 3000,
      headers: {
        "x-api-key": key,
        "anthropic-version": "2023-06-01",
      },
    }));
  } catch (_) {
    return null;
  }
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

// A throw, a non-zero exit and unparsable output are the same fact: we could
// not ask. None of them is the CLI answering no.
function probeInstalledCli(): CliProbe {
  let raw: string;
  try {
    raw = host.providerVersionJson();
  } catch (_) {
    return { kind: "unreachable" };
  }
  const result = safeJson(raw) as { status?: string; version?: string } | null;
  if (!result || result.status !== "detected") return { kind: "unreachable" };
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(String(result.version || ""));
  if (!match) return { kind: "unreachable" };
  return { kind: "version", version: [Number(match[1]), Number(match[2]), Number(match[3])] };
}

function applyInstalledCapabilities(models: ModelInfo[], probe: CliProbe, liveDiscovery = false): ModelInfo[] {
  return models.map((model) => {
    const declaredMinimum = manifest()?.cliCompatibility?.modelMinimumCli?.[model.id];
    const minimum = declaredMinimum?.split(".").map(Number);
    if (!minimum) return { ...model };
    const verdict = decideAvailability(probe, minimum, liveDiscovery ? model.available : undefined);
    // An unknown verdict publishes no boolean at all: a boolean here is what
    // the cache would freeze and the picker would grey out.
    if (verdict === "unknown") {
      const unknown: ModelInfo = { ...model, availability: "unknown" };
      delete unknown.available;
      return unknown;
    }
    return { ...model, available: verdict === "available", availability: verdict };
  });
}

function atLeast(version: number[], minimum: number[]): boolean {
  for (let i = 0; i < minimum.length; i++) {
    const actual = version[i] || 0;
    if (actual !== minimum[i]) return actual > minimum[i];
  }
  return true;
}

function mapAliasesToLive(aliases: ModelInfo[], live: AnthropicModel[]): ModelInfo[] {
  const out: ModelInfo[] = [];
  for (let i = 0; i < aliases.length; i++) {
    const alias = aliases[i];
    const family = familyOf(alias);
    const match = family ? bestLiveMatch(live, family) : null;
    const mapped: ModelInfo = {
      ...alias,
      id: alias.id,
      displayName: match && alias.primary && liveDisplay(match)
        ? liveDisplay(match)
        : match && !alias.id.endsWith("[1m]")
        ? withVersion(alias.displayName, versionOf(match))
        : alias.displayName,
    };
    if (alias.description) mapped.description = alias.description;
    mapped.available = !!match;
    out.push(mapped);
  }
  return out;
}

function familyOf(alias: ModelInfo): string | null {
  const id = alias.id.toLowerCase();
  if (id === "best") return "fable";
  if (id === "opusplan") return "opus";
  const hay = (id.replace(/\[1m\]$/, "") + " " + alias.displayName).toLowerCase();
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

// word and any suffix like "(1M context)"; append if there's no version token.
function withVersion(displayName: string, version: string): string {
  if (!version) return displayName;
  if (/\d+(?:\.\d+)?/.test(displayName)) return displayName.replace(/\d+(?:\.\d+)?/, version);
  return displayName + " " + version;
}

function safeJson(s: string | null | undefined): unknown {
  try { return JSON.parse(String(s || "null")); } catch (_) { return null; }
}
