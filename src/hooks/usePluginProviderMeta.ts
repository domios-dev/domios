/**
 * Mirrors the plugin snapshot (`plugin_manager_snapshot`) into the react-free
 * `pluginProviderMeta` cache so the pane resolver can light up shell panes by
 * `backing.plugin_id`. Refetches when plugin contributions change (install /
 * activate / reload). One mount at app root is enough.
 */
import { useEffect } from "react";
import { invoke, listen } from "../lib/bridge";
import {
  pluginProviderMetaFromSnapshot,
  setPluginProviderMeta,
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
