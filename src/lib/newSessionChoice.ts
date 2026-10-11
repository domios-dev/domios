import type { ProviderDescriptor } from "./types";

export const NEW_SESSION_CHOICE_STORAGE_KEY = "codeterm:new-session-choice";
export const OPEN_NEW_SESSION_EVENT = "codeterm:open-new-session";

export interface OpenNewSessionDetail {
  workspaceId: string;
  workspaceCwd: string | null;
  initialProviderId?: string;
  section?: NewSessionSection;
}

export function openNewSessionPicker(detail: OpenNewSessionDetail): void {
  window.dispatchEvent(new CustomEvent<OpenNewSessionDetail>(OPEN_NEW_SESSION_EVENT, { detail }));
}

export type NewSessionMode = "single" | "pair";
export type NewSessionSection = "general" | "workspace";

export function allowedNewSessionModes(section: NewSessionSection): readonly NewSessionMode[] {
  return section === "general" ? ["single"] : ["single", "pair"];
}

export function choiceForSessionSection(choice: NewSessionChoice, section: NewSessionSection): NewSessionChoice {
  if (allowedNewSessionModes(section).includes(choice.mode) || choice.mode === "single") return choice;
  return {
    mode: "single",
    providerId: choice.orchestratorProviderId,
    modelId: choice.orchestratorModelId,
    effortId: choice.orchestratorEffortId,
  };
}

export interface SingleSessionChoice {
  mode: "single";
  providerId: string;
  modelId?: string;
  effortId?: string;
  preset?: string;
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

export function reconcilePresetSelection(
  choice: NewSessionChoice,
  presets: ReadonlyArray<{ provider: string; name: string; model: string }>,
): NewSessionChoice {
  if (choice.mode !== "single" || !choice.preset || presets.some((preset) =>
    preset.provider === choice.providerId && preset.name === choice.preset && preset.model === choice.modelId)) return choice;
  return { ...choice, preset: undefined };
}

function resolveProvider(
  providers: ProviderDescriptor[],
  selectedProviderId?: string,
): ProviderDescriptor | undefined {
  return providers.find((provider) => provider.id === selectedProviderId) ?? providers[0];
}

export function resolveProviderModel(
  provider: Pick<ProviderDescriptor, "available_models" | "default_model"> | undefined,
  selectedModelId?: string,
): string | undefined {
  const models = provider?.available_models ?? [];
  if (models.some((model) => model.id === selectedModelId)) return selectedModelId;
  if (models.some((model) => model.id === provider?.default_model)) return provider?.default_model;
  return models[0]?.id;
}

export function resolveEffort(
  provider: ProviderDescriptor | undefined,
  modelId?: string,
  selectedEffortId?: string,
): string | undefined {
  const efforts = reasoningEffortsForModel(provider, modelId);
  if (efforts.length === 0) return undefined;
  const model = provider?.available_models.find(
    (candidate) => candidate.id.toLowerCase() === modelId?.toLowerCase(),
  );
  if (efforts.some((effort) => effort.id === selectedEffortId)) return selectedEffortId;
  if (efforts.some((effort) => effort.id === model?.default_reasoning_effort)) {
    return model?.default_reasoning_effort;
  }
  if (efforts.some((effort) => effort.id === provider?.default_reasoning_effort)) {
    return provider?.default_reasoning_effort;
  }
  return efforts[0]?.id;
}

export function reasoningEffortsForModel(
  provider: ProviderDescriptor | undefined,
  modelId?: string,
): NonNullable<ProviderDescriptor["reasoning_efforts"]> {
  const normalizedModelId = modelId?.toLowerCase();
  const model = provider?.available_models.find(
    (candidate) => candidate.id.toLowerCase() === normalizedModelId,
  );
  return model?.reasoning_efforts ?? provider?.reasoning_efforts ?? [];
}

export function defaultNewSessionChoice(providers: ProviderDescriptor[]): NewSessionChoice {
  const provider = providers[0];
  const providerId = provider?.id ?? "";
  const modelId = resolveProviderModel(provider);
  return {
    mode: "pair",
    orchestratorProviderId: providerId,
    orchestratorModelId: modelId,
    orchestratorEffortId: resolveEffort(provider, modelId),
    plannerProviderId: providerId,
    plannerModelId: modelId,
    plannerEffortId: resolveEffort(provider, modelId),
  };
}

export function resolvePersistedChoice(
  providers: ProviderDescriptor[],
  choice: NewSessionChoice,
): NewSessionChoice {
  if (choice.mode === "single") {
    const provider = resolveProvider(providers, choice.providerId);
    const modelId = resolveProviderModel(provider, choice.modelId);
    return {
      mode: "single",
      ...(choice.preset && provider?.id === choice.providerId && modelId === choice.modelId ? { preset: choice.preset } : {}),
      providerId: provider?.id ?? "",
      modelId,
      effortId: resolveEffort(provider, modelId, choice.effortId),
    };
  }
  const orchestrator = resolveProvider(providers, choice.orchestratorProviderId);
  const planner = resolveProvider(providers, choice.plannerProviderId);
  const orchestratorModelId = resolveProviderModel(orchestrator, choice.orchestratorModelId);
  const plannerModelId = resolveProviderModel(planner, choice.plannerModelId);
  return {
    mode: "pair",
    orchestratorProviderId: orchestrator?.id ?? "",
    orchestratorModelId,
    orchestratorEffortId: resolveEffort(orchestrator, orchestratorModelId, choice.orchestratorEffortId),
    plannerProviderId: planner?.id ?? "",
    plannerModelId,
    plannerEffortId: resolveEffort(planner, plannerModelId, choice.plannerEffortId),
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
