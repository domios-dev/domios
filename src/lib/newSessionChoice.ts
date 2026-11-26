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

export type NewSessionMode = "single" | "pair" | "terminal";
export type NewSessionSection = "general" | "workspace";

export function allowedNewSessionModes(section: NewSessionSection): readonly NewSessionMode[] {
  return section === "general" ? ["single"] : ["single", "pair", "terminal"];
}

export function unavailableProviderIds(
  providers: ReadonlyArray<Pick<ProviderDescriptor, "id" | "availability">>,
  installMap: Readonly<Record<string, boolean>>,
): Set<string> {
  return new Set(
    providers
      .filter((provider) => installMap[provider.id] === false || (installMap[provider.id] !== true && provider.availability === "unavailable"))
      .map((provider) => provider.id),
  );
}

export function isProviderSelectable(
  provider: Pick<ProviderDescriptor, "id" | "availability">,
  disabledProviderIds?: ReadonlySet<string>,
): boolean {
  return disabledProviderIds ? !disabledProviderIds.has(provider.id) : provider.availability !== "unavailable";
}

export function firstSelectableProvider(
  providers: readonly ProviderDescriptor[],
  disabledProviderIds?: ReadonlySet<string>,
): ProviderDescriptor | undefined {
  return providers.find((provider) => isProviderSelectable(provider, disabledProviderIds));
}

export function choiceForSessionSection(
  choice: NewSessionChoice,
  section: NewSessionSection,
  providers: ProviderDescriptor[] = [],
  disabledProviderIds?: ReadonlySet<string>,
): NewSessionChoice {
  if (allowedNewSessionModes(section).includes(choice.mode) || choice.mode === "single") return choice;
  if (choice.mode === "terminal") {
    const provider = firstSelectableProvider(providers, disabledProviderIds);
    const modelId = resolveProviderModel(provider);
    return {
      mode: "single",
      providerId: provider?.id ?? "",
      modelId,
      effortId: resolveEffort(provider, modelId),
    };
  }
  const single: SingleSessionChoice = {
    mode: "single",
    providerId: choice.orchestratorProviderId,
    modelId: choice.orchestratorModelId,
    effortId: choice.orchestratorEffortId,
  };
  return providers.length > 0 ? resolvePersistedChoice(providers, single, disabledProviderIds) : single;
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

export interface TerminalSessionChoice {
  mode: "terminal";
  shellProfile?: string;
}

export type NewSessionChoice = SingleSessionChoice | PairSessionChoice | TerminalSessionChoice;

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
  disabledProviderIds?: ReadonlySet<string>,
): ProviderDescriptor | undefined {
  return providers.find(
    (provider) => provider.id === selectedProviderId && isProviderSelectable(provider, disabledProviderIds),
  ) ?? firstSelectableProvider(providers, disabledProviderIds);
}

export function resolveProviderModel(
  provider: Pick<ProviderDescriptor, "available_models" | "default_model"> | undefined,
  selectedModelId?: string,
): string | undefined {
  const models = (provider?.available_models ?? []).filter((model) => model.available !== false);
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

/** A picker opened for one provider starts on a model that can launch, never an unavailable default. */
export function initialProviderChoice(provider: ProviderDescriptor): NewSessionChoice {
  const modelId = resolveProviderModel(provider);
  return { mode: "single", providerId: provider.id, modelId, effortId: resolveEffort(provider, modelId) };
}

export function defaultNewSessionChoice(
  providers: ProviderDescriptor[],
  disabledProviderIds?: ReadonlySet<string>,
): NewSessionChoice {
  const provider = firstSelectableProvider(providers, disabledProviderIds);
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
  disabledProviderIds?: ReadonlySet<string>,
): NewSessionChoice {
  if (choice.mode === "terminal") {
    return choice.shellProfile ? { mode: "terminal", shellProfile: choice.shellProfile } : { mode: "terminal" };
  }
  if (choice.mode === "single") {
    const provider = resolveProvider(providers, choice.providerId, disabledProviderIds);
    const modelId = resolveProviderModel(provider, choice.modelId);
    return {
      mode: "single",
      ...(choice.preset && provider?.id === choice.providerId && modelId === choice.modelId ? { preset: choice.preset } : {}),
      providerId: provider?.id ?? "",
      modelId,
      effortId: resolveEffort(provider, modelId, choice.effortId),
    };
  }
  const orchestrator = resolveProvider(providers, choice.orchestratorProviderId, disabledProviderIds);
  const planner = resolveProvider(providers, choice.plannerProviderId, disabledProviderIds);
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
  disabledProviderIds?: ReadonlySet<string>,
): NewSessionChoice {
  try {
    const raw = window.localStorage.getItem(NEW_SESSION_CHOICE_STORAGE_KEY);
    if (!raw) return defaultNewSessionChoice(providers, disabledProviderIds);
    const parsed = JSON.parse(raw) as NewSessionChoice;
    if (parsed.mode === "terminal") {
      return resolvePersistedChoice(providers, parsed, disabledProviderIds);
    }
    if (parsed.mode === "single" && parsed.providerId) {
      return resolvePersistedChoice(providers, parsed, disabledProviderIds);
    }
    if (
      parsed.mode === "pair"
      && parsed.orchestratorProviderId
      && parsed.plannerProviderId
    ) {
      return resolvePersistedChoice(providers, parsed, disabledProviderIds);
    }
  } catch {
    // Quota/private-mode or corrupt payload — fall back to defaults.
  }
  return defaultNewSessionChoice(providers, disabledProviderIds);
}

export function writePersistedNewSessionChoice(choice: NewSessionChoice): void {
  try {
    window.localStorage.setItem(NEW_SESSION_CHOICE_STORAGE_KEY, JSON.stringify(choice));
  } catch {
    // Best-effort UI prefs.
  }
}
