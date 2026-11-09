import type { RuntimeAskScope, RuntimeSettings, RuntimeServiceTierSelection, RuntimeServiceTier } from "@codeterm/plugin-sdk";
import type { ProviderDescriptor } from "./types";
import { reasoningEffortsForModel } from "./newSessionChoice";

export type PickerCategory = "model" | "effort" | "mode" | "speed";
export type PickerApplyTarget = { kind: "config"; id: string } | { kind: "session" | "host_approvals" | PickerCategory };
export type HostApprovalsValue = "ask" | "allow_all";
export interface PickerSection {
  id: string;
  label: string;
  category?: PickerCategory;
  kind: "select";
  options: readonly { value: string; label: string; disabled?: boolean }[];
  current: string;
  applyTarget: PickerApplyTarget;
}

const CATEGORIES = new Map<string, PickerCategory>([["model", "model"], ["thought_level", "effort"], ["mode", "mode"]]);
const ORDER: readonly PickerCategory[] = ["model", "effort", "mode", "speed"];
const ASK_LABELS: Readonly<Record<RuntimeAskScope, string>> = { outsideWorkspace: "Ask outside workspace" };

/**
 * The tab's host-approval choice, or `undefined` when the control does not apply: the agent's own
 * approve-all mode is the single switch where it exists, and confinement owns the answer where it binds.
 */
export function hostApprovalsChoice(
  descriptor: ProviderDescriptor | undefined,
  config: Readonly<Record<string, unknown>> | null | undefined,
  controls: RuntimeSettings | undefined,
): HostApprovalsValue | undefined {
  if (!descriptor?.agent_runtime?.hostApprovals || config?.hostApprovals === "locked") return undefined;
  const native = declaredFullAccessMode(descriptor, config);
  if (native && (!controls || sessionOffersMode(controls, native))) return undefined;
  return config?.hostApprovals === "ask" ? "ask" : "allow_all";
}

/**
 * True when the host gives full access while the agent's mode row reads otherwise: the agent refused its
 * approve-all mode, or left it. The row alone would mislead, so the picker states the effective access.
 */
export function hostFullAccessOverride(
  descriptor: ProviderDescriptor | undefined,
  config: Readonly<Record<string, unknown>> | null | undefined,
  controls: RuntimeSettings | undefined,
): boolean {
  const native = descriptor?.agent_runtime?.hostApprovals ? declaredFullAccessMode(descriptor, config) : undefined;
  if (!native || !controls || config?.hostApprovals !== "allow_all" || !sessionOffersMode(controls, native)) return false;
  return (modeOption(controls)?.currentValue ?? controls.mode) !== native;
}

function tabLaunchProfile(descriptor: ProviderDescriptor | undefined, config: Readonly<Record<string, unknown>> | null | undefined) {
  return typeof config?.launchProfile === "string" ? descriptor?.agent_runtime?.launchProfiles?.[config.launchProfile] : undefined;
}

function declaredFullAccessMode(descriptor: ProviderDescriptor, config: Readonly<Record<string, unknown>> | null | undefined): string | undefined {
  return tabLaunchProfile(descriptor, config)?.fullAccessMode;
}

/** How far the tab's agent asks, so the Ask choice never promises more questions than the agent sends. */
export function declaredAskScope(
  descriptor: ProviderDescriptor | undefined,
  config: Readonly<Record<string, unknown>> | null | undefined,
): RuntimeAskScope | undefined {
  return tabLaunchProfile(descriptor, config)?.askScope;
}

function modeOption(controls: RuntimeSettings) {
  return controls.configOptions?.find(option => option.category === "mode");
}

function sessionOffersMode(controls: RuntimeSettings, mode: string): boolean {
  return Boolean(modeOption(controls)?.options.some(option => option.value === mode) || controls.modes?.some(entry => entry.id === mode));
}

/** The names the agent gave its modes this session; a stopped session no longer declares them. */
export function sessionModeNames(controls: RuntimeSettings | undefined): readonly (readonly [string, string])[] {
  return [
    ...(controls?.modes ?? []).map(mode => [mode.id, mode.name] as const),
    ...(controls ? modeOption(controls)?.options ?? [] : []).map(option => [option.value, option.name] as const),
  ];
}

const RECOVERING_MODE_UNKNOWN = "Unknown while recovering";

/**
 * What the identity chip reads while no live session reports a mode: the owner's last choice, or an honest
 * unknown once the session is down. `null` means a live session answers, so the chip reads as it normally does.
 */
export function recoveringModeLabel(
  runtimeState: "loading" | "loginPending" | "loaded" | "unloaded" | "degraded" | null | undefined,
  config: Readonly<Record<string, unknown>> | null | undefined,
  names: ReadonlyMap<string, string>,
): string | null {
  if (runtimeState !== "degraded" && runtimeState !== "unloaded" && runtimeState !== "loading") return null;
  const chosen = [config?.sessionMode, config?.fullAccessMode, config?.mode]
    .find((mode): mode is string => typeof mode === "string" && mode.trim().length > 0);
  if (chosen) return names.get(chosen) ?? chosen;
  return runtimeState === "loading" ? null : RECOVERING_MODE_UNKNOWN;
}

export function pickerSections({ descriptor, controls, model, effort, runtimeMode, serviceTier, serviceTierAvailability, hostApprovals, hostAskScope, hostFullAccess }: {
  descriptor?: ProviderDescriptor;
  controls?: RuntimeSettings;
  model?: string;
  effort?: string;
  runtimeMode?: string;
  serviceTier?: RuntimeServiceTierSelection;
  serviceTierAvailability?: Record<string, readonly RuntimeServiceTier[]>;
  hostApprovals?: HostApprovalsValue;
  hostAskScope?: RuntimeAskScope;
  hostFullAccess?: boolean;
}): PickerSection[] {
  const known = new Map<PickerCategory, PickerSection>();
  const extras: PickerSection[] = [];
  for (const option of controls?.configOptions ?? []) {
    const category = option.category ? CATEGORIES.get(option.category) : undefined;
    const section: PickerSection = {
      id: option.id, label: option.name, category, kind: "select", current: option.currentValue,
      options: option.options.map(value => ({ value: value.value, label: value.name })),
      applyTarget: { kind: "config", id: option.id },
    };
    if (category) { if (!known.has(category)) known.set(category, section); }
    else extras.push(section);
  }
  if (!known.has("model") && descriptor?.available_models.length) known.set("model", {
    id: "model", label: "Model", category: "model", kind: "select", current: model ?? "",
    options: descriptor.available_models.map(value => ({ value: value.id, label: value.display_name, disabled: value.availability === "unavailable" || value.available === false })),
    applyTarget: { kind: "model" },
  });
  const efforts = reasoningEffortsForModel(descriptor, model);
  if (!known.has("effort") && efforts.length) known.set("effort", {
    id: "effort", label: "Reasoning", category: "effort", kind: "select", current: effort ?? "",
    options: efforts.map(value => ({ value: value.id, label: value.display_name })), applyTarget: { kind: "effort" },
  });
  if (!known.has("mode")) {
    if (controls?.modes?.length) known.set("mode", {
      id: "session-mode", label: "Mode", category: "mode", kind: "select", current: controls.mode ?? "",
      options: controls.modes.map(mode => ({ value: mode.id, label: mode.name })), applyTarget: { kind: "session" },
    });
    else if (descriptor?.agent_runtime?.runtimeModes?.length) known.set("mode", {
      id: "runtime-mode", label: "Runtime mode", category: "mode", kind: "select", current: runtimeMode ?? "",
      options: descriptor.agent_runtime.runtimeModes.map(mode => ({ value: mode.id, label: mode.displayName })), applyTarget: { kind: "mode" },
    });
  }
  if (descriptor?.agent_runtime?.serviceTiers?.length) {
    const tiers = (model ? serviceTierAvailability?.[model] : undefined) ?? descriptor.agent_runtime.serviceTiers;
    known.set("speed", {
      id: "speed", label: "Speed", category: "speed", kind: "select", current: JSON.stringify(serviceTier?.id ?? null),
      options: [{ value: JSON.stringify(null), label: "Default" }, ...tiers.map(tier => ({ value: JSON.stringify(tier.id), label: tier.name, disabled: Boolean(tier.unavailableReason) }))],
      applyTarget: { kind: "speed" },
    });
  }
  const permissions: PickerSection[] = hostApprovals ? [{
    id: "host-approvals", label: "Permissions", kind: "select", current: hostApprovals,
    options: [{ value: "ask", label: hostAskScope ? ASK_LABELS[hostAskScope] : "Ask when the agent asks" }, { value: "allow_all", label: "Allow all" }],
    applyTarget: { kind: "host_approvals" },
  }] : hostFullAccess ? [{
    id: "host-approvals", label: "Permissions", kind: "select", current: "Full access (host)", options: [],
    applyTarget: { kind: "host_approvals" },
  }] : [];
  return [...ORDER.flatMap(category => known.has(category) ? [known.get(category)!] : []), ...permissions, ...extras];
}

export function pickerDescriptor(descriptor: ProviderDescriptor, sections: readonly PickerSection[]): ProviderDescriptor {
  const model = sections.find(section => section.category === "model");
  const effort = sections.find(section => section.category === "effort");
  if (model?.applyTarget.kind !== "config") return descriptor.agent_runtime && !descriptor.available_models.length && sections.some(section => ["config", "session", "host_approvals"].includes(section.applyTarget.kind))
    ? { ...descriptor, model_switch: "live" } : descriptor;
  return { ...descriptor, model_switch: "live", available_models: model.options.map(option => ({
    id: option.value, display_name: option.label,
    reasoning_efforts: effort?.options.map(value => ({ id: value.value, display_name: value.label })),
  })) };
}
