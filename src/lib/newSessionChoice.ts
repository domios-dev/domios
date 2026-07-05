import type { ProviderDescriptor } from "./types";

export const NEW_SESSION_CHOICE_STORAGE_KEY = "codeterm:new-session-choice";

export type NewSessionMode = "single" | "pair";

export interface SingleSessionChoice {
  mode: "single";
  providerId: string;
  modelId?: string;
}

export interface PairSessionChoice {
  mode: "pair";
  orchestratorProviderId: string;
  orchestratorModelId?: string;
  plannerProviderId: string;
  plannerModelId?: string;
}

export type NewSessionChoice = SingleSessionChoice | PairSessionChoice;

function pickModel(provider: ProviderDescriptor | undefined, preferred: string): string | undefined {
  if (!provider) return undefined;
  const models = provider.available_models ?? [];
  const exact = models.find((m) => m.id === preferred);
  if (exact) return exact.id;
  const fuzzy = models.find((m) => m.id.includes(preferred) || m.display_name.toLowerCase().includes(preferred));
  return fuzzy?.id ?? provider.default_model;
}

export function defaultNewSessionChoice(providers: ProviderDescriptor[]): NewSessionChoice {
  const claude = providers.find((p) => p.id === "claude") ?? providers[0];
  const providerId = claude?.id ?? "";
  return {
    mode: "pair",
    orchestratorProviderId: providerId,
    orchestratorModelId: pickModel(claude, "opus"),
    plannerProviderId: providerId,
    plannerModelId: pickModel(claude, "fable"),
  };
}

export function readPersistedNewSessionChoice(
  providers: ProviderDescriptor[],
): NewSessionChoice {
  try {
    const raw = window.localStorage.getItem(NEW_SESSION_CHOICE_STORAGE_KEY);
    if (!raw) return defaultNewSessionChoice(providers);
    const parsed = JSON.parse(raw) as NewSessionChoice;
    if (parsed.mode === "single" && parsed.providerId) return parsed;
    if (
      parsed.mode === "pair"
      && parsed.orchestratorProviderId
      && parsed.plannerProviderId
    ) {
      return parsed;
    }
  } catch {
    // Quota/private-mode or corrupt payload — fall back to defaults.
  }
  return defaultNewSessionChoice(providers);
}

export function writePersistedNewSessionChoice(choice: NewSessionChoice): void {
  try {
    window.localStorage.setItem(NEW_SESSION_CHOICE_STORAGE_KEY, JSON.stringify(choice));
  } catch {
    // Best-effort UI prefs.
  }
}
