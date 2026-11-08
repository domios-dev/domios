import { tabMetaParts } from "../../lib/tabMetaLabel";
import { pickerControlOf } from "../../lib/runtimeDeclaredControls";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
import type { RuntimeServiceTierSelection as ServiceTierSelection } from "../../../packages/plugin-sdk/src/agentRuntime";
import { modelSwitchModeSupported } from "../../lib/modelSwitch";
import { agentModelForBadge } from "../../lib/agentLabel";
import { reasoningEffortsForModel, resolveEffort } from "../../lib/newSessionChoice";
import { groupOptionsByLabel } from "../../lib/optionGroups";
import type { ModelInfo, ProviderDescriptor, ReasoningEffortInfo, Tab } from "../../lib/types";

export type SwitchTiming = "live" | "live_command" | "next_turn" | "restart_resume";

export type StatusLineSwitchGate =
  | { interactive: true; timing: SwitchTiming; confirmRestart: boolean; note: string }
  | { interactive: false; reason: string };

export interface StatusLineTabFacts {
  hasSession: boolean;
}

export interface PickerModelRow {
  id: string;
  label: string;
  badge?: string;
  current: boolean;
  unavailable: boolean;
}

export interface PickerModelGroup {
  label: string;
  rows: PickerModelRow[];
}

export interface PickerSelection {
  model: string;
  effort?: string;
  serviceTier?: ServiceTierSelection;
  runtimeMode?: string;
}

export function pickerSelection(descriptor: ProviderDescriptor, model: string, effort?: string, serviceTier?: ServiceTierSelection, runtimeMode?: string): PickerSelection {
  return {
    model,
    effort,
    ...(descriptor.agent_runtime?.serviceTiers && serviceTier !== undefined ? { serviceTier } : {}),
    ...(descriptor.agent_runtime?.runtimeModes?.length && runtimeMode !== undefined ? { runtimeMode } : {}),
  };
}

export type PickerPlan =
  | { action: "none" }
  | { action: "apply"; selection: PickerSelection }
  | { action: "confirm"; selection: PickerSelection };

export const PICKER_SEARCH_THRESHOLD = 8;

export function pickerHasPrimaryModels(descriptor: ProviderDescriptor): boolean {
  return descriptor.available_models.some((model) => model.primary === true);
}

/**
 * A runtime agent's status-line label reads like every other provider's: model · effort, with the agent's
 * logo beside it. The agent's own name and version are not part of the label (see `runtimeAgentTooltip`).
 */
export function runtimeStatusLineIdentity(state: Pick<AgentRuntimeState, "controls"> | null | undefined, tierLabel?: string, effort?: string, status?: string): string {
  const model = state?.controls?.configOptions?.find(option => pickerControlOf(option) === "model");
  const name = model?.options.find(option => option.value === model.currentValue)?.name ?? model?.currentValue;
  return tabMetaParts(name, tierLabel, effort, status).join(" · ");
}

/** The agent's reported name and version, for the label's tooltip. */
export function runtimeAgentTooltip(identity: { agentName?: string; binaryVersion?: string } | null | undefined): string | undefined {
  return [identity?.agentName, identity?.binaryVersion].filter(Boolean).join(" · ") || undefined;
}

export function pickerHarnessName(descriptor: ProviderDescriptor, identity?: { agentName?: string } | null): string {
  return descriptor.agent_runtime?.launchProfiles ? identity?.agentName?.trim() || descriptor.display_name : descriptor.display_name;
}

export function statusLineModelName(
  tab: Pick<Tab, "agent_session_model" | "agent_provider_resolved_model" | "agent_requested_model">,
  fallback?: string | null,
): string {
  return agentModelForBadge({
    sessionModel: tab.agent_session_model,
    providerResolvedModel: tab.agent_provider_resolved_model,
    requestedModel: tab.agent_requested_model,
  }) ?? (fallback?.trim() || "Unknown model");
}

export function switchTimingNote(timing: SwitchTiming): string {
  if (timing === "live_command" || timing === "live") return "Applies now";
  if (timing === "next_turn") return "Applies to the next turn";
  return "Restarts the session";
}

export function statusLineSwitchGate(
  descriptor: ProviderDescriptor | undefined,
  facts: StatusLineTabFacts,
): StatusLineSwitchGate {
  if (!descriptor) return { interactive: false, reason: "Model choices are still loading" };
  const mode = descriptor.model_switch;
  if (!modelSwitchModeSupported(mode)) {
    return {
      interactive: false,
      reason: `${descriptor.display_name} picks its model when a session starts, so it can't switch mid-session`,
    };
  }
  if (!facts.hasSession) return { interactive: false, reason: "Start the agent to change its model" };
  const timing = mode as SwitchTiming;
  return { interactive: true, timing, confirmRestart: timing === "restart_resume", note: switchTimingNote(timing) };
}

export function matchesModel(model: ModelInfo, candidate: string | null | undefined): boolean {
  const needle = candidate?.trim().toLowerCase();
  if (!needle) return false;
  return model.id.toLowerCase() === needle || model.display_name.toLowerCase() === needle;
}

export function currentModelId(
  descriptor: ProviderDescriptor | undefined,
  candidates: Array<string | null | undefined>,
): string | undefined {
  const candidate = candidates.find((candidate) => candidate?.trim())?.trim();
  const model = descriptor?.available_models.find((entry) => matchesModel(entry, candidate));
  return model?.id ?? candidate;
}

// The model a live session runs stays selectable even when the catalogue has not caught up.
function withSessionModel(models: ModelInfo[], sessionModel: string | undefined): ModelInfo[] {
  const id = sessionModel?.trim();
  if (!id || models.some((model) => matchesModel(model, id))) return models;
  return [...models, { id, display_name: id, primary: true }];
}

function isUnavailable(model: ModelInfo): boolean {
  return model.availability === "unavailable" || (model.availability == null && model.available === false);
}

function matchesQuery(model: ModelInfo, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return [model.id, model.display_name, model.group ?? ""].some((text) => text.toLowerCase().includes(needle));
}

function prefixGroupLabel(descriptor: ProviderDescriptor, model: ModelInfo): string {
  const groups = descriptor.model_groups ?? [];
  return groups.find((group) => model.id.startsWith(`${group.id}/`))?.label ?? "";
}

export function pickerModelGroups(
  descriptor: ProviderDescriptor,
  currentModel: string | undefined,
  query = "",
  showAll = false,
  sessionModel: string | undefined = currentModel,
): PickerModelGroup[] {
  const curated = pickerHasPrimaryModels(descriptor) && !showAll && !query.trim();
  const models = withSessionModel(descriptor.available_models, sessionModel).filter((model) =>
    (!curated || model.primary === true) && matchesQuery(model, query));
  const toRow = (model: ModelInfo): PickerModelRow => ({
    id: model.id,
    label: model.display_name || model.id,
    badge: model.badge ?? undefined,
    current: matchesModel(model, currentModel),
    unavailable: isUnavailable(model),
  });
  const declared = groupOptionsByLabel(models);
  if (declared) return declared.map((group) => ({ label: group.label, rows: group.options.map(toRow) }));
  const routed = groupOptionsByLabel(models.map((model) => ({ model, group: prefixGroupLabel(descriptor, model) })));
  if (routed) {
    return routed.map((group) => ({ label: group.label, rows: group.options.map((entry) => toRow(entry.model)) }));
  }
  const featured = models.filter((model) => model.featured === true);
  const rest = models.filter((model) => model.featured !== true);
  return models.length === 0 ? [] : [{ label: "", rows: [...featured, ...rest].map(toRow) }];
}

export function pickerNeedsSearch(descriptor: ProviderDescriptor): boolean {
  return descriptor.available_models.length > PICKER_SEARCH_THRESHOLD;
}

export function effortChoices(descriptor: ProviderDescriptor, modelId: string | undefined): ReasoningEffortInfo[] {
  return reasoningEffortsForModel(descriptor, modelId);
}

export function effortAfterModelChange(
  descriptor: ProviderDescriptor,
  modelId: string,
  currentEffort: string | undefined,
): string | undefined {
  return resolveEffort(descriptor, modelId, currentEffort);
}

export function planSelection(
  gate: StatusLineSwitchGate,
  current: { model?: string; effort?: string; serviceTier?: ServiceTierSelection; runtimeMode?: string },
  next: PickerSelection,
): PickerPlan {
  if (!gate.interactive) return { action: "none" };
  const sameModel = current.model?.toLowerCase() === next.model.toLowerCase();
  if (sameModel && current.serviceTier?.id === next.serviceTier?.id && (current.effort ?? undefined) === (next.effort ?? undefined) && current.runtimeMode === next.runtimeMode) return { action: "none" };
  return { action: gate.confirmRestart ? "confirm" : "apply", selection: next };
}

