export type EndpointKind = "openai-compatible" | "anthropic";

export interface ModelEndpoint {
  id: string;
  kind: EndpointKind;
  baseUrl: string;
  models: string[];
  name?: string;
  secretName?: string;
}

export type EndpointFormMode = "add" | "edit";
export type KeySource = "new" | "saved" | "keep";

export interface EndpointDraft {
  id: string;
  kind: EndpointKind;
  baseUrl: string;
  keySource: KeySource;
  keyValue: string;
  savedSecret: string;
  discovered: string[] | null;
  selected: string[];
}

interface ProviderWire {
  id: string;
  kind: EndpointKind;
  baseUrl: string;
  models: string[];
}

export type AddRequest =
  | { action: "add"; provider: ProviderWire; value: string }
  | { action: "add_from_secret"; provider: ProviderWire; source_secret: string };

export type UpdateRequest = { action: "update"; provider: ProviderWire; value?: string };

export type DiscoverRequest =
  | { action: "discover"; kind: EndpointKind; base_url: string; value: string }
  | { action: "discover"; kind: EndpointKind; base_url: string; source_secret: string };

export type DiscoverPlan =
  | { kind: "request"; request: DiscoverRequest }
  | { kind: "stored"; id: string }
  | { kind: "blocked"; reason: string };

export const MANAGED_SECRET_PREFIX = "domios/model-provider/";

const ID_PATTERN = /^[a-z][a-z0-9]*(-[a-z0-9]+)*$/;

export function emptyDraft(): EndpointDraft {
  return {
    id: "",
    kind: "openai-compatible",
    baseUrl: "",
    keySource: "new",
    keyValue: "",
    savedSecret: "",
    discovered: null,
    selected: [],
  };
}

export function draftFromEndpoint(endpoint: ModelEndpoint): EndpointDraft {
  return {
    ...emptyDraft(),
    id: endpoint.id,
    kind: endpoint.kind,
    baseUrl: endpoint.baseUrl,
    keySource: "keep",
    discovered: [...endpoint.models],
    selected: [...endpoint.models],
  };
}

export function managedSecretName(id: string): string {
  return `${MANAGED_SECRET_PREFIX}${id.trim()}`;
}

export function isHttpUrl(value: string): boolean {
  try {
    const url = new URL(value.trim());
    return (url.protocol === "http:" || url.protocol === "https:") && !!url.hostname && !url.search && !url.hash;
  } catch {
    return false;
  }
}

export function endpointHost(baseUrl: string): string {
  try {
    const url = new URL(baseUrl);
    const path = url.pathname.replace(/\/+$/, "");
    return `${url.host}${path}`;
  } catch {
    return baseUrl;
  }
}

export function chosenModels(draft: EndpointDraft): string[] {
  const discovered = draft.discovered ?? [];
  return discovered.filter((model) => draft.selected.includes(model));
}

function providerWire(draft: EndpointDraft): ProviderWire {
  return { id: draft.id.trim(), kind: draft.kind, baseUrl: draft.baseUrl.trim(), models: chosenModels(draft) };
}

export function draftProblem(draft: EndpointDraft, mode: EndpointFormMode): string | null {
  if (mode === "add" && !ID_PATTERN.test(draft.id.trim())) {
    return "Name: start with a letter; use lowercase letters, digits and single dashes.";
  }
  if (!isHttpUrl(draft.baseUrl)) return "Base URL: an http:// or https:// address without a query, e.g. http://localhost:1234/v1.";
  if (draft.keySource === "new" && mode === "add" && !draft.keyValue) return "API key: paste the key, or pick a saved secret.";
  if (draft.keySource === "saved" && !draft.savedSecret) return "API key: pick a saved secret.";
  return null;
}

export function addRequest(draft: EndpointDraft): AddRequest {
  const provider = providerWire(draft);
  if (draft.keySource === "saved") {
    return { action: "add_from_secret", provider, source_secret: draft.savedSecret };
  }
  return { action: "add", provider, value: draft.keyValue };
}

export function updateRequest(draft: EndpointDraft): UpdateRequest {
  const provider = providerWire(draft);
  return draft.keySource === "new" && draft.keyValue
    ? { action: "update", provider, value: draft.keyValue }
    : { action: "update", provider };
}

export function discoverPlan(draft: EndpointDraft, original?: ModelEndpoint): DiscoverPlan {
  if (!isHttpUrl(draft.baseUrl)) return { kind: "blocked", reason: "Enter the base URL first." };
  const base = { action: "discover" as const, kind: draft.kind, base_url: draft.baseUrl.trim() };
  if (draft.keySource === "new" && draft.keyValue) {
    return { kind: "request", request: { ...base, value: draft.keyValue } };
  }
  if (draft.keySource === "saved" && draft.savedSecret) {
    return { kind: "request", request: { ...base, source_secret: draft.savedSecret } };
  }
  if (draft.keySource === "keep" && original) {
    const unchanged = original.baseUrl === draft.baseUrl.trim() && original.kind === draft.kind;
    return unchanged
      ? { kind: "stored", id: original.id }
      : { kind: "blocked", reason: "Enter the key again to discover models at the new address, or save first." };
  }
  return { kind: "blocked", reason: "Add the API key first." };
}

export function applyDiscovered(draft: EndpointDraft, models: string[]): EndpointDraft {
  const unique = [...new Set(models)];
  const kept = draft.selected.filter((model) => unique.includes(model));
  return { ...draft, discovered: unique, selected: kept };
}

export function toggleModel(draft: EndpointDraft, model: string): EndpointDraft {
  const selected = draft.selected.includes(model)
    ? draft.selected.filter((item) => item !== model)
    : [...draft.selected, model];
  return { ...draft, selected };
}

export function selectableSecrets(names: readonly string[]): string[] {
  return [...new Set(names)].filter((name) => !name.startsWith(MANAGED_SECRET_PREFIX)).sort((a, b) => a.localeCompare(b));
}

const ERROR_LINES: Record<string, string> = {
  connection: "Couldn't reach the endpoint. Check the base URL and that the server is running.",
  timeout: "The endpoint didn't answer in time. Check the base URL and that the server is running.",
  unauthorized: "The endpoint rejected the key. Check the API key.",
  invalid_response: "The endpoint answered without a model list. The base URL usually ends at the API root, such as /v1.",
  invalid_request: "Check the fields: a lowercase name, an http(s) base URL without a query, and a key.",
  conflict: "An endpoint with this name already exists. Pick another name.",
  not_found: "That endpoint or saved secret no longer exists. Refresh and try again.",
  secret_locked: "Secrets are locked. Unlock them in Extensions, then try again.",
  secret_unavailable: "The secret store is unavailable. Check the Secrets backend in Extensions.",
  secret_ownership: "Domios does not manage this endpoint's key. Remove the endpoint and add it again.",
  busy: "The harness config is being changed by another operation. Try again in a moment.",
  config: "Couldn't read or write the harness config file. Check that it is valid JSON.",
  cleanup_failed: "The change failed and could not be fully rolled back. Check the endpoint list.",
  unsupported: "This harness does not accept custom model endpoints.",
  ambiguous: "More than one harness accepts custom endpoints; open the endpoint list from the harness page.",
};

export function errorKind(error: unknown): string | null {
  if (typeof error === "string") return /^[a-z_]+$/.test(error) ? error : null;
  if (error && typeof error === "object") {
    const kind = (error as { kind?: unknown }).kind;
    if (typeof kind === "string") return kind;
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") return /"kind"\s*:\s*"([a-z_]+)"/.exec(message)?.[1] ?? null;
  }
  return null;
}

export function errorLine(error: unknown): string {
  const kind = errorKind(error);
  return (kind && ERROR_LINES[kind]) ?? "Something went wrong talking to the endpoint. Try again.";
}

export function modelCountLabel(count: number): string {
  return count === 1 ? "1 model" : `${count} models`;
}
