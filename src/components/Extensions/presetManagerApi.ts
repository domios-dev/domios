/**
 * Data seam for the chatBackend preset manager (R5b).
 *
 * Presets are user-owned config, not plugin-baked: they live at
 * `config.plugins.<id>.presets` and the active selection at
 * `config.plugins.<id>.activePreset`. The plugin's `listPresets()` reads the
 * same slot back, so editing here is what the backend sees. Persistence goes
 * through `get_config`/`update_config`; applying a preset to a LIVE session
 * (its bound model) reuses the chatBackend `switch_model` command. Generalized
 * — keyed by `pluginId`, never a hardcoded provider id.
 */
import { invoke } from "../../lib/bridge";

/** A user-defined preset for a chatBackend. Mirrors the plugin SDK `PresetInfo`
 *  plus the editable behaviour (system prompt, bound model, params). */
export interface Preset {
  id: string;
  name: string;
  systemPrompt?: string;
  /** R4 binding: the model this preset drives. Absent ⇒ inherit the backend default. */
  model?: string;
  /** Free-form sampling params (e.g. `{ temperature: 0.7 }`). */
  params?: Record<string, unknown>;
}

export interface PresetState {
  presets: Preset[];
  activePreset: string | null;
}

export interface PresetManagerApi {
  load(pluginId: string): Promise<PresetState>;
  save(pluginId: string, state: PresetState): Promise<void>;
  /** Apply a preset's bound model to a live session (best-effort). */
  applyToSession(pluginId: string, sessionId: string, model: string): Promise<void>;
}

interface PluginConfigShape {
  plugins?: Record<string, Record<string, unknown> | undefined>;
  [k: string]: unknown;
}

const defaultApi: PresetManagerApi = {
  load: async (pluginId) => {
    const cfg = await invoke<PluginConfigShape>("get_config");
    const slot = cfg?.plugins?.[pluginId] ?? {};
    return {
      presets: Array.isArray(slot.presets) ? (slot.presets as Preset[]) : [],
      activePreset: typeof slot.activePreset === "string" ? slot.activePreset : null,
    };
  },
  save: async (pluginId, state) => {
    const cfg = await invoke<PluginConfigShape>("get_config");
    const plugins = { ...(cfg.plugins ?? {}) };
    plugins[pluginId] = {
      ...(plugins[pluginId] ?? {}),
      presets: state.presets,
      activePreset: state.activePreset,
    };
    await invoke("update_config", { config: { ...cfg, plugins } });
  },
  applyToSession: async (pluginId, sessionId, model) => {
    await invoke("chatbackend_switch_model", { pluginId, sessionId, model });
  },
};

let apiOverride: PresetManagerApi | null = null;

export function getPresetManagerApi(): PresetManagerApi {
  return apiOverride ?? defaultApi;
}

export function __setPresetManagerApiForTest(api: PresetManagerApi | null): void {
  apiOverride = api;
}
