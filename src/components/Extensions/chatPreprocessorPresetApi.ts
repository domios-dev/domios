/**
 * Data seam for chatPreprocessor plugin presets. Stored at
 * `config.plugins.<id>.presets` / `.activePreset` — the same config bag the
 * runtime plugin reads through `host.settingsJson()`. Keyed by `pluginId`, no
 * hardcoded id.
 */
import { invoke } from "../../lib/bridge";

/** A user-created preprocessing configuration. The bundled plugin reads the
 * active preset and config from the same plugin settings slot. */
export interface PreprocessorPreset {
  id: string;
  name: string;
  kind?: "template" | "mem-recall" | string;
  /** Human-readable description shown in the picker bubble. */
  description?: string;
  /** Template presets support `{{text}}` and `{{mem}}`. */
  template?: string;
  /** Plugin-defined config passed through to the preset runtime. */
  config?: Record<string, unknown>;
  /** Built-in presets are shown and activatable, but not saved as user presets. */
  system?: boolean;
}

export interface PreprocessorPresetState {
  presets: PreprocessorPreset[];
  activePreset: string | null;
}

export interface PreprocessorPresetApi {
  load(pluginId: string): Promise<PreprocessorPresetState>;
  save(pluginId: string, state: PreprocessorPresetState): Promise<void>;
}

interface PluginConfigShape {
  plugins?: Record<string, Record<string, unknown> | undefined>;
  [k: string]: unknown;
}

const defaultApi: PreprocessorPresetApi = {
  load: async (pluginId) => {
    const cfg = await invoke<PluginConfigShape>("get_config");
    const slot = cfg?.plugins?.[pluginId] ?? {};
    return {
      presets: Array.isArray(slot.presets)
        ? (slot.presets as PreprocessorPreset[])
        : [],
      activePreset:
        typeof slot.activePreset === "string"
          ? slot.activePreset
          : null,
    };
  },
  save: async (pluginId, state) => {
    const cfg = await invoke<PluginConfigShape>("get_config");
    const plugins = { ...(cfg.plugins ?? {}) };
    plugins[pluginId] = {
      ...(plugins[pluginId] ?? {}),
      presets: state.presets.filter((p) => !p.system),
      activePreset: state.activePreset,
    };
    await invoke("update_config", { config: { ...cfg, plugins } });
  },
};

let apiOverride: PreprocessorPresetApi | null = null;

export function getPreprocessorPresetApi(): PreprocessorPresetApi {
  return apiOverride ?? defaultApi;
}

export function __setPreprocessorPresetApiForTest(api: PreprocessorPresetApi | null): void {
  apiOverride = api;
}
