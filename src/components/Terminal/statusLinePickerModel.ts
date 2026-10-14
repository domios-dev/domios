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
}

export type PickerPlan =
  | { action: "none" }
  | { action: "apply"; selection: PickerSelection }
  | { action: "confirm"; selection: PickerSelection };

export const PICKER_SEARCH_THRESHOLD = 8;

export function pickerHasPrimaryModels(descriptor: ProviderDescriptor): boolean {
  return descriptor.available_models.some((model) => model.primary === true);
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
): PickerModelGroup[] {
  const curated = pickerHasPrimaryModels(descriptor) && !showAll && !query.trim();
  const models = descriptor.available_models.filter((model) =>
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
  current: { model?: string; effort?: string },
  next: PickerSelection,
): PickerPlan {
  if (!gate.interactive) return { action: "none" };
  const sameModel = current.model?.toLowerCase() === next.model.toLowerCase();
  if (sameModel && (current.effort ?? undefined) === (next.effort ?? undefined)) return { action: "none" };
  return { action: gate.confirmRestart ? "confirm" : "apply", selection: next };
}

/** Opens upward from the trigger, kept inside the viewport horizontally. */
export function pickerAnchor(
  trigger: { left: number; top: number },
  viewport: { width: number; height: number },
  width: number,
  margin = 8,
): { left: number; bottom: number } {
  const left = Math.max(margin, Math.min(trigger.left, viewport.width - width - margin));
  return { left, bottom: Math.max(margin, viewport.height - trigger.top + 6) };
}
