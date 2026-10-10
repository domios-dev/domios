import type { SettingBlockHost, SettingBlockState, SettingControl, SettingValue } from "@codeterm/chat-ui";
import { buildCatalog, type CatalogEntry } from "../../components/Extensions/pluginCatalog";
import type { AppConfigSource } from "../appConfigSource";
import type { PluginSnapshotStoreState } from "../pluginSnapshotStore";
import type { ProviderDescriptor } from "../types";
import { settingBlockState, splitSettingKey, type SchemaLookup, type SettingNamespace } from "./settingBlockState";

export interface SettingBlockDeps {
  config: Pick<AppConfigSource, "get" | "subscribe" | "ensure" | "set">;
  plugins: {
    get(): PluginSnapshotStoreState;
    subscribe(listener: () => void): () => void;
    ensure(): Promise<unknown>;
  };
  agents: {
    list(): readonly ProviderDescriptor[];
    subscribe(listener: () => void): () => void;
    /** Loads the provider descriptors once when nothing has loaded them yet. */
    ensure(): Promise<void>;
  };
  openPlugin(pluginId: string): void;
}

/** A value the control could have produced; anything else never reaches the config. */
export function acceptsValue(control: SettingControl, value: SettingValue): boolean {
  switch (control.kind) {
    case "switch":
      return typeof value === "boolean";
    case "choice":
      return typeof value === "string" && control.options.some((option) => option.value === value);
    case "stepper":
      return typeof value === "number" && Number.isFinite(value)
        && (control.min === undefined || value >= control.min) && (control.max === undefined || value <= control.max);
    case "text":
      return typeof value === "string";
  }
}

/** Setting card services: reads follow config and schema changes, writes take the config set path after a click. */
export function createSettingBlockHost(deps: SettingBlockDeps): SettingBlockHost {
  let catalogFor: PluginSnapshotStoreState | null = null;
  let catalog: CatalogEntry[] = [];
  let agentsState: "idle" | "loading" | "done" | { error: string } = "idle";
  let inputs: unknown[] = [];
  const states = new Map<string, SettingBlockState>();

  const pluginLookup = (pluginId: string): SchemaLookup => {
    const state = deps.plugins.get();
    if (state !== catalogFor) {
      catalogFor = state;
      catalog = buildCatalog(state.snapshot);
    }
    const entry = catalog.find((candidate) => candidate.id === pluginId);
    if (entry) return { status: "ready", owner: entry.displayName, schema: entry.settingsSchema ?? [] };
    if (!state.hasLoaded) return state.error ? { status: "unavailable", message: state.error } : { status: "loading" };
    return { status: "missing" };
  };
  const agentLookup = (providerId: string): SchemaLookup => {
    const descriptor = deps.agents.list().find((candidate) => candidate.id === providerId);
    if (descriptor) return { status: "ready", owner: descriptor.display_name, schema: descriptor.settings_schema ?? [] };
    if (typeof agentsState === "object") return { status: "unavailable", message: agentsState.error };
    return agentsState === "done" || deps.agents.list().length > 0 ? { status: "missing" } : { status: "loading" };
  };
  const lookup = (namespace: SettingNamespace, ownerId: string) => (namespace === "plugins" ? pluginLookup(ownerId) : agentLookup(ownerId));

  const read = (key: string): SettingBlockState => {
    const config = deps.config.get();
    const next = [config, deps.plugins.get(), deps.agents.list(), agentsState];
    if (next.some((value, index) => value !== inputs[index])) {
      inputs = next;
      states.clear();
    }
    let state = states.get(key);
    if (!state) {
      state = settingBlockState({ key, config: config.config, configError: config.error, lookup });
      states.set(key, state);
    }
    return state;
  };

  const notifyAgents = new Set<() => void>();
  const loadAgents = () => {
    if (agentsState !== "idle" || deps.agents.list().length > 0) return;
    agentsState = "loading";
    deps.agents.ensure().then(
      () => { agentsState = "done"; },
      (error: unknown) => { agentsState = { error: error instanceof Error ? error.message : String(error) }; },
    ).finally(() => {
      for (const listener of notifyAgents) listener();
    });
  };

  return {
    binding: {
      subscribe(listener) {
        const stops = [deps.config.subscribe(listener), deps.plugins.subscribe(listener), deps.agents.subscribe(listener)];
        notifyAgents.add(listener);
        return () => {
          notifyAgents.delete(listener);
          for (const stop of stops) stop();
        };
      },
      read,
      refresh(key) {
        void deps.config.ensure().catch(() => {});
        const parts = splitSettingKey(key);
        if (parts?.namespace === "plugins") void deps.plugins.ensure().catch(() => {});
        if (parts?.namespace === "agents") loadAgents();
      },
    },
    async write(key, value) {
      const state = read(key);
      if (state.kind !== "ready") throw new Error("this setting is not available here");
      if (!acceptsValue(state.setting.control, value)) throw new Error("that value is not one this setting accepts");
      await deps.config.set(key, value);
    },
    openSettings(key) {
      const parts = splitSettingKey(key);
      if (parts) deps.openPlugin(parts.ownerId);
    },
  };
}
