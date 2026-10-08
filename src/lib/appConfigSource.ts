import { mergeConfigEvent } from "../hooks/useConfig";
import { invoke, listen } from "./bridge";
import type { AppConfig } from "./types";
import { daemonFetch } from "./webApi";

export interface AppConfigSnapshot {
  config: AppConfig | null;
  error: string | null;
}

export interface AppConfigSourceDeps {
  load(): Promise<AppConfig>;
  /** Delivers every daemon-side config change; returns the unsubscribe. */
  onChange(listener: (config: AppConfig) => void): Promise<() => void>;
  patch(config: AppConfig, key: string, value: unknown): Promise<void>;
}

export interface AppConfigSource {
  get(): AppConfigSnapshot;
  subscribe(listener: () => void): () => void;
  /** Loads unless a subscriber keeps the snapshot current through `config-changed`. */
  ensure(): Promise<void>;
  /** The `codeterm config set` path: one keyed PATCH that the daemon validates, saves and broadcasts. */
  set(key: string, value: unknown): Promise<void>;
}

/** Live read-only view of the daemon config for surfaces outside the React config hook. */
export function createAppConfigSource(deps: AppConfigSourceDeps): AppConfigSource {
  let snapshot: AppConfigSnapshot = { config: null, error: null };
  let fresh = false;
  let loading: Promise<void> | null = null;
  let listening = false;
  let unlisten: (() => void) | null = null;
  const listeners = new Set<() => void>();
  const publish = (next: AppConfigSnapshot) => {
    snapshot = next;
    for (const listener of listeners) listener();
  };
  const onChange = (incoming: AppConfig) => {
    fresh = true;
    publish({ config: snapshot.config ? mergeConfigEvent(snapshot.config, incoming) : incoming, error: null });
  };
  const ensure = () => {
    if (fresh) return Promise.resolve();
    loading ??= deps.load().then(
      (config) => {
        fresh = listening;
        publish({ config, error: null });
      },
      (error: unknown) => publish({ config: snapshot.config, error: error instanceof Error ? error.message : String(error) }),
    ).finally(() => {
      loading = null;
    });
    return loading;
  };
  return {
    get: () => snapshot,
    subscribe(listener) {
      listeners.add(listener);
      if (!listening) {
        listening = true;
        void deps.onChange(onChange).then((stop) => {
          if (listening) unlisten = stop;
          else stop();
        }, () => {
          listening = false;
        });
      }
      return () => {
        listeners.delete(listener);
        if (listeners.size > 0) return;
        listening = false;
        fresh = false;
        unlisten?.();
        unlisten = null;
      };
    },
    ensure,
    async set(key, value) {
      await ensure();
      const config = snapshot.config;
      if (!config) throw new Error(snapshot.error ?? "config is unavailable");
      const write = configWrite(config, key, value);
      await deps.patch(config, write.key, write.value);
    },
  };
}

/** The daemon sets a key only under an existing object, so a missing parent is written whole at its first missing ancestor. */
export function configWrite(config: unknown, key: string, value: unknown): { key: string; value: unknown } {
  const parts = key.split(".");
  let current: unknown = config;
  for (let index = 0; index < parts.length - 1; index++) {
    const next = isRecord(current) ? current[parts[index]] : undefined;
    if (!isRecord(next)) {
      const nested = parts.slice(index + 1).reduceRight<unknown>((inner, part) => ({ [part]: inner }), value);
      return { key: parts.slice(0, index + 1).join("."), value: nested };
    }
    current = next;
  }
  return { key, value };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

async function patchConfig(config: AppConfig, key: string, value: unknown): Promise<void> {
  const response = await daemonFetch(config.api, "/api/config", {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ key, value }),
  });
  if (!response.ok) throw new Error((await response.text()).trim() || `HTTP ${response.status}`);
}

export const appConfigSource = createAppConfigSource({
  load: async () => {
    const config = await invoke<AppConfig>("get_config");
    if (!config) throw new Error("config is unavailable");
    return config;
  },
  onChange: (listener) => listen<AppConfig>("config-changed", (event) => listener(event.payload)),
  patch: patchConfig,
});
