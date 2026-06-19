// Model discovery: live Anthropic API list for API-key users, manifest seed
// otherwise. Backs the provider's discoverModels method.
import type { ModelInfo } from "@codeterm/plugin-sdk";
import { manifest } from "./shared";

interface AnthropicModelsResponse { data?: AnthropicModel[] }
interface AnthropicModel { id?: string; display_name?: string; displayName?: string }

export function discoverAnthropicModels(): ModelInfo[] | null {
  const key = host.envGet("ANTHROPIC_API_KEY") || host.envGet("CLAUDE_API_KEY");
  if (!key) return null;
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
  const models: ModelInfo[] = [];
  for (let i = 0; i < data.length; i++) {
    const item = data[i];
    if (!item) continue;
    const id = typeof item.id === "string" ? item.id.trim() : "";
    if (!id) continue;
    const display = typeof item.display_name === "string" && item.display_name.trim()
      ? item.display_name.trim()
      : (typeof item.displayName === "string" && item.displayName.trim()
          ? item.displayName.trim()
          : displayNameFromId(id));
    models.push({ id: id, displayName: display });
  }
  return models.length ? models : null;
}

export function manifestModels(): ModelInfo[] {
  const m = manifest();
  return Array.isArray(m && m.models) ? m!.models! : [];
}

function safeJson(s: string | null | undefined): unknown {
  try { return JSON.parse(String(s || "null")); } catch (_) { return null; }
}

function displayNameFromId(id: string): string {
  const leaf = id.split("/").pop() || id;
  return leaf.split("-").filter(Boolean).map((part) => {
    return part ? part.charAt(0).toUpperCase() + part.substring(1) : part;
  }).join(" ");
}
