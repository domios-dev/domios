/**
 * Data seam for the chatBackend preset manager. Presets are user-owned config at
 * `config.plugins.<id>.presets` (active at `.activePreset`) — the same slot
 * `listPresets()` reads back. Persists via `get_config`/`update_config`; applying
 * to a live session reuses `switch_model`. Keyed by `pluginId`, no hardcoded id.
 */
import { invoke } from "../../lib/bridge";
import { setCachedShellModel } from "../../lib/shellSessionModel";

type ChatSessionInfo = import("../../../gen/ChatSessionInfo").ChatSessionInfo;

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

interface PendingPresetSave {
  state: PresetState;
  waiters: Array<{ resolve: () => void; reject: (error: unknown) => void }>;
}

const saveQueues = new Map<string, { inFlight: boolean; pending: PendingPresetSave | null }>();

async function writePresetState(pluginId: string, state: PresetState): Promise<void> {
  const cfg = await invoke<PluginConfigShape>("get_config");
  const plugins = { ...(cfg.plugins ?? {}) };
  plugins[pluginId] = {
    ...(plugins[pluginId] ?? {}),
    presets: state.presets,
    activePreset: state.activePreset,
  };
  await invoke("update_config", { config: { ...cfg, plugins } });
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
    let queue = saveQueues.get(pluginId);
    if (!queue) {
      queue = { inFlight: false, pending: null };
      saveQueues.set(pluginId, queue);
    }
    if (queue.inFlight) {
      return await new Promise<void>((resolve, reject) => {
        if (queue.pending) {
          queue.pending.state = state;
          queue.pending.waiters.push({ resolve, reject });
        } else {
          queue.pending = { state, waiters: [{ resolve, reject }] };
        }
      });
    }
    queue.inFlight = true;
    const drainPending = async () => {
      try {
        while (queue.pending) {
          const next = queue.pending;
          queue.pending = null;
          try {
            await writePresetState(pluginId, next.state);
            for (const waiter of next.waiters) waiter.resolve();
          } catch (error) {
            for (const waiter of next.waiters) waiter.reject(error);
          }
        }
      } finally {
        queue.inFlight = false;
        if (!queue.pending) saveQueues.delete(pluginId);
      }
    };
    try {
      await writePresetState(pluginId, state);
    } finally {
      if (queue.pending) {
        void drainPending();
      } else {
        queue.inFlight = false;
        saveQueues.delete(pluginId);
      }
    }
  },
  applyToSession: async (pluginId, sessionId, model) => {
    const info = await invoke<ChatSessionInfo | null>("chatbackend_switch_model", { pluginId, sessionId, model });
    setCachedShellModel(pluginId, sessionId, info?.model ?? model);
  },
};

let apiOverride: PresetManagerApi | null = null;

export function getPresetManagerApi(): PresetManagerApi {
  return apiOverride ?? defaultApi;
}

export function __setPresetManagerApiForTest(api: PresetManagerApi | null): void {
  apiOverride = api;
}
