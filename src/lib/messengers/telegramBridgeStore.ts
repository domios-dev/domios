import { useEffect, useSyncExternalStore } from "react";
import { appConfigSource } from "../appConfigSource";
import { IS_WEB } from "../webApi";
import { connectTelegram, telegramBridgeApi, type TelegramBridgeApi, type TelegramBridgeRaw } from "./telegramBridge";

export type TelegramBridgeSnapshot =
  | { kind: "loading" }
  | { kind: "unavailable"; message: string }
  | { kind: "ready"; raw: TelegramBridgeRaw };

export interface TelegramBridgeStore {
  get(): TelegramBridgeSnapshot;
  subscribe(listener: () => void): () => void;
  /** Re-reads the bridge state; never changes it. */
  refresh(): Promise<void>;
  connect(token?: string): Promise<void>;
  disconnect(): Promise<void>;
}

/** One live view of the Telegram bridge shared by Settings and chat blocks; it re-reads whenever config changes. */
export function createTelegramBridgeStore(api: TelegramBridgeApi, watchConfig: (listener: () => void) => () => void): TelegramBridgeStore {
  let snapshot: TelegramBridgeSnapshot = { kind: "loading" };
  let reading: Promise<void> | null = null;
  let stopWatching: (() => void) | null = null;
  const listeners = new Set<() => void>();
  const publish = (next: TelegramBridgeSnapshot) => {
    snapshot = next;
    for (const listener of listeners) listener();
  };
  const refresh = () => {
    reading ??= api.state().then(
      (raw) => publish({ kind: "ready", raw }),
      (error: unknown) => {
        if (snapshot.kind !== "ready") publish({ kind: "unavailable", message: error instanceof Error ? error.message : String(error) });
      },
    ).finally(() => {
      reading = null;
    });
    return reading;
  };
  const onWake = () => void refresh();
  const afterwards = async (run: () => Promise<void>) => {
    try {
      await run();
    } finally {
      await refresh();
    }
  };
  return {
    get: () => snapshot,
    subscribe(listener) {
      listeners.add(listener);
      if (!stopWatching) {
        const unwatch = watchConfig(onWake);
        window.addEventListener("focus", onWake);
        stopWatching = () => {
          unwatch();
          window.removeEventListener("focus", onWake);
        };
      }
      return () => {
        listeners.delete(listener);
        if (listeners.size > 0) return;
        stopWatching?.();
        stopWatching = null;
      };
    },
    refresh,
    connect: (token) => afterwards(() => connectTelegram(api, token)),
    disconnect: () => afterwards(() => api.stop()),
  };
}

export const telegramBridgeStore = createTelegramBridgeStore(
  telegramBridgeApi(async () => {
    // The browser mirror always talks to its own daemon, so only the desktop needs the configured API address.
    if (IS_WEB) return {};
    await appConfigSource.ensure();
    const config = appConfigSource.get().config;
    if (!config) throw new Error(appConfigSource.get().error ?? "config is unavailable");
    return config.api;
  }),
  (listener) => appConfigSource.subscribe(listener),
);

export function useTelegramBridge(store: TelegramBridgeStore = telegramBridgeStore): TelegramBridgeSnapshot {
  const snapshot = useSyncExternalStore(store.subscribe, store.get, store.get);
  useEffect(() => {
    void store.refresh();
  }, [store]);
  return snapshot;
}
