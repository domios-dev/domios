/**
 * Mirrors the plugin snapshot (`plugin_manager_snapshot`) into the react-free
 * `pluginProviderMeta` cache so the tab resolver can light up shell tabs by
 * `backing.plugin_id`. Refetches when plugin contributions change (install /
 * activate / reload). One mount at app root is enough.
 */
import { useCallback, useEffect, useSyncExternalStore } from "react";
import { invoke, listen } from "../lib/bridge";
import {
  pluginDisplayName,
  pluginProviderMetaFromSnapshot,
  pluginProviderMetaRecord,
  setPluginProviderMeta,
  subscribePluginProviderMeta,
  type PluginSnapshotLike,
} from "../lib/pluginProviderMeta";

export function usePluginProviderMetaSync(): void {
  useEffect(() => {
    let alive = true;
    const refresh = () => {
      invoke<PluginSnapshotLike>("plugin_manager_snapshot")
        .then((snapshot) => {
          if (alive) setPluginProviderMeta(pluginProviderMetaFromSnapshot(snapshot));
        })
        .catch((e) => console.warn("plugin provider meta sync failed:", e));
    };
    refresh();
    let unlisten: (() => void) | undefined;
    void listen("contributions-changed", refresh).then((un) => {
      if (alive) unlisten = un;
      else un();
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
}

/** Resolves installed plugin ids to display names; a new identity whenever the plugin list changes. */
export function usePluginDisplayNameResolver(): (pluginId: string) => string | undefined {
  const record = useSyncExternalStore(subscribePluginProviderMeta, pluginProviderMetaRecord, pluginProviderMetaRecord);
  return useCallback((pluginId: string) => pluginDisplayName(record, pluginId), [record]);
}
