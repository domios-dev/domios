import type { RuntimeConfigOption, RuntimeSettings } from "@codeterm/plugin-sdk";
import type { AgentRuntimeState } from "./agentRuntimeState";
import type { Tab } from "./types";

export interface SnapshotRuntimeAssets {
  icons?: Record<string, string>;
  authentication?: Record<string, NonNullable<AgentRuntimeState["authentication"]>>;
  commands?: Record<string, NonNullable<RuntimeSettings["commands"]>>;
  modes?: Record<string, NonNullable<RuntimeSettings["modes"]>>;
  configOptions?: Record<string, readonly Omit<RuntimeConfigOption, "currentValue">[]>;
  serviceTierAvailability?: Record<string, NonNullable<RuntimeSettings["serviceTierAvailability"]>>;
}

type WireRuntimeControls = RuntimeSettings & {
  commandsRef?: string;
  modesRef?: string;
  configOptionsRef?: string;
  configOptionValues?: readonly string[];
  serviceTierAvailabilityRef?: string;
};

type WireAgentRuntimeState = AgentRuntimeState & {
  iconRef?: string;
  authenticationRef?: string;
  controls?: WireRuntimeControls;
};

type WireAgentRuntimeConfig = NonNullable<Tab["agent_runtime_config"]> & {
  serviceTierAvailabilityRef?: string;
};

function hydrateControls(
  controls: WireRuntimeControls,
  assets: SnapshotRuntimeAssets,
): RuntimeSettings {
  const {
    commandsRef,
    modesRef,
    configOptionsRef,
    configOptionValues,
    serviceTierAvailabilityRef,
    ...rest
  } = controls;
  const next: RuntimeSettings = { ...rest };
  if (commandsRef !== undefined) {
    const commands = assets.commands?.[commandsRef];
    if (commands !== undefined) next.commands = commands;
  }
  if (modesRef !== undefined) {
    const modes = assets.modes?.[modesRef];
    if (modes !== undefined) next.modes = modes;
  }
  if (serviceTierAvailabilityRef !== undefined) {
    const tiers = assets.serviceTierAvailability?.[serviceTierAvailabilityRef];
    if (tiers !== undefined) next.serviceTierAvailability = tiers;
  }
  if (configOptionsRef !== undefined) {
    const template = assets.configOptions?.[configOptionsRef];
    if (template !== undefined) {
      next.configOptions = template.map((option, index) => ({
        ...option,
        currentValue: configOptionValues?.[index] ?? "",
      }));
    }
  }
  return next;
}

function hydrateRuntimeState(
  state: WireAgentRuntimeState,
  assets: SnapshotRuntimeAssets,
): AgentRuntimeState {
  const { iconRef, authenticationRef, controls, ...rest } = state;
  const hasControlRefs = Boolean(
    controls && (
      controls.commandsRef !== undefined
      || controls.modesRef !== undefined
      || controls.configOptionsRef !== undefined
      || controls.serviceTierAvailabilityRef !== undefined
    ),
  );
  if (iconRef === undefined && authenticationRef === undefined && !hasControlRefs) {
    return state;
  }
  const next: AgentRuntimeState = { ...rest };
  if (iconRef !== undefined) {
    const iconSvg = assets.icons?.[iconRef];
    if (iconSvg !== undefined) next.iconSvg = iconSvg;
  }
  if (authenticationRef !== undefined) {
    const authentication = assets.authentication?.[authenticationRef];
    if (authentication !== undefined) next.authentication = authentication;
  }
  if (controls) {
    next.controls = hasControlRefs ? hydrateControls(controls, assets) : controls;
  }
  return next;
}

function hydrateRuntimeConfig(
  config: WireAgentRuntimeConfig,
  assets: SnapshotRuntimeAssets,
): NonNullable<Tab["agent_runtime_config"]> {
  if (config.serviceTierAvailabilityRef === undefined) return config;
  const { serviceTierAvailabilityRef, ...rest } = config;
  const tiers = assets.serviceTierAvailability?.[serviceTierAvailabilityRef];
  return tiers !== undefined ? { ...rest, serviceTierAvailability: tiers } : rest;
}

/**
 * Expands content-addressed `runtime_assets` from a wire `/api/mux/state`
 * snapshot back onto each tab while sharing identical asset references across
 * tabs. Returns the input unchanged when `runtime_assets` is absent.
 */
export function hydrateMuxSnapshot<T>(snapshot: T): T {
  if (!snapshot || typeof snapshot !== "object" || !("runtime_assets" in snapshot)) {
    return snapshot;
  }
  const wire = snapshot as T & {
    runtime_assets?: SnapshotRuntimeAssets;
    tabs?: Record<string, Tab>;
  };
  const assets = wire.runtime_assets;
  if (!assets || typeof assets !== "object") {
    return snapshot;
  }
  const { runtime_assets: _, ...rest } = wire;
  if (!rest.tabs || typeof rest.tabs !== "object") {
    return rest as T;
  }
  const tabs: Record<string, Tab> = {};
  for (const [id, tab] of Object.entries(rest.tabs)) {
    if (!tab || typeof tab !== "object") {
      tabs[id] = tab;
      continue;
    }
    const nextState = tab.agent_runtime_state
      ? hydrateRuntimeState(tab.agent_runtime_state as WireAgentRuntimeState, assets)
      : tab.agent_runtime_state;
    const nextConfig = tab.agent_runtime_config
      ? hydrateRuntimeConfig(tab.agent_runtime_config as WireAgentRuntimeConfig, assets)
      : tab.agent_runtime_config;
    tabs[id] = nextState === tab.agent_runtime_state && nextConfig === tab.agent_runtime_config
      ? tab
      : { ...tab, agent_runtime_state: nextState, agent_runtime_config: nextConfig };
  }
  return { ...rest, tabs } as T;
}
