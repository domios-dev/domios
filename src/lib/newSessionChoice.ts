import type { ProviderDescriptor } from "./types";

export const NEW_SESSION_CHOICE_STORAGE_KEY = "codeterm:new-session-choice";
export const OPEN_NEW_SESSION_EVENT = "codeterm:open-new-session";

export interface OpenNewSessionDetail {
  workspaceId: string;
  workspaceCwd: string | null;
  initialProviderId?: string;
}

export function openNewSessionPicker(detail: OpenNewSessionDetail): void {
  window.dispatchEvent(new CustomEvent<OpenNewSessionDetail>(OPEN_NEW_SESSION_EVENT, { detail }));
}

export type NewSessionMode = "single" | "pair";

export interface SingleSessionChoice {
  mode: "single";
  providerId: string;
  modelId?: string;
  effortId?: string;
}

export interface PairSessionChoice {
  mode: "pair";
  orchestratorProviderId: string;
  orchestratorModelId?: string;
  orchestratorEffortId?: string;
  plannerProviderId: string;
  plannerModelId?: string;
  plannerEffortId?: string;
}

export type NewSessionChoice = SingleSessionChoice | PairSessionChoice;

function resolveProvider(
  providers: ProviderDescriptor[],
  selectedProviderId?: string,
): ProviderDescriptor | undefined {
  return providers.find((provider) => provider.id === selectedProviderId) ?? providers[0];
}

function resolveModel(
  provider: ProviderDescriptor | undefined,
  selectedModelId?: string,
): string | undefined {
  const models = provider?.available_models ?? [];
  if (models.some((model) => model.id === selectedModelId)) return selectedModelId;
  if (models.some((model) => model.id === provider?.default_model)) return provider?.default_model;
  return models[0]?.id;
}

export function resolveEffort(
  provider: ProviderDescriptor | undefined,
  selectedEffortId?: string,
): string | undefined {
  const efforts = provider?.reasoning_efforts ?? [];
  if (efforts.length === 0) return undefined;
  if (efforts.some((effort) => effort.id === selectedEffortId)) return selectedEffortId;
  if (efforts.some((effort) => effort.id === provider?.default_reasoning_effort)) {
    return provider?.default_reasoning_effort;
  }
  return efforts[0]?.id;
}

export function defaultNewSessionChoice(providers: ProviderDescriptor[]): NewSessionChoice {
  const provider = providers[0];
  const providerId = provider?.id ?? "";
  const modelId = resolveModel(provider);
  return {
    mode: "pair",
    orchestratorProviderId: providerId,
    orchestratorModelId: modelId,
    orchestratorEffortId: resolveEffort(provider),
    plannerProviderId: providerId,
    plannerModelId: modelId,
    plannerEffortId: resolveEffort(provider),
  };
}

function resolvePersistedChoice(
  providers: ProviderDescriptor[],
  choice: NewSessionChoice,
): NewSessionChoice {
  if (choice.mode === "single") {
    const provider = resolveProvider(providers, choice.providerId);
    return {
      mode: "single",
      providerId: provider?.id ?? "",
      modelId: resolveModel(provider, choice.modelId),
      effortId: resolveEffort(provider, choice.effortId),
    };
  }
  const orchestrator = resolveProvider(providers, choice.orchestratorProviderId);
  const planner = resolveProvider(providers, choice.plannerProviderId);
  return {
    mode: "pair",
    orchestratorProviderId: orchestrator?.id ?? "",
    orchestratorModelId: resolveModel(orchestrator, choice.orchestratorModelId),
    orchestratorEffortId: resolveEffort(orchestrator, choice.orchestratorEffortId),
    plannerProviderId: planner?.id ?? "",
    plannerModelId: resolveModel(planner, choice.plannerModelId),
    plannerEffortId: resolveEffort(planner, choice.plannerEffortId),
  };
}

export function readPersistedNewSessionChoice(
  providers: ProviderDescriptor[],
): NewSessionChoice {
  try {
    const raw = window.localStorage.getItem(NEW_SESSION_CHOICE_STORAGE_KEY);
    if (!raw) return defaultNewSessionChoice(providers);
    const parsed = JSON.parse(raw) as NewSessionChoice;
    if (parsed.mode === "single" && parsed.providerId) {
      return resolvePersistedChoice(providers, parsed);
    }
    if (
      parsed.mode === "pair"
      && parsed.orchestratorProviderId
      && parsed.plannerProviderId
    ) {
      return resolvePersistedChoice(providers, parsed);
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
