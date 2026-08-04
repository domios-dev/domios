import { useEffect, useReducer, useSyncExternalStore } from "react";
import { listen } from "./bridge";
import { subscribeAgentDescriptors } from "./agentDescriptorCache";
import { subscribePluginProviderMeta } from "./pluginProviderMeta";
import { resolveTabProvider, type ResolvedTabProvider } from "./tabProvider";
import { fetchShellSessionModel, getCachedShellModel, subscribeShellModel } from "./shellSessionModel";
import type { Tab } from "./types";

/** Reactive `resolveTabProvider`: re-resolves when either the descriptor cache
 *  or the plugin-snapshot cache changes, so a tab lights up the moment its
 *  provider metadata arrives. */
export function useTabProvider(tab: Tab | null | undefined): ResolvedTabProvider | null {
  const [, bump] = useReducer((c: number) => c + 1, 0);
  useEffect(() => {
    const a = subscribeAgentDescriptors(bump);
    const b = subscribePluginProviderMeta(bump);
    return () => {
      a();
      b();
    };
  }, []);
  return resolveTabProvider(tab);
}

/** Live model of a chatBackend shell session (fetched via
 *  `chatbackend_session_info`, cached per session, refreshed on
 *  `mux-state-changed`). Null for non-shell tabs. Memo-safe: no fetch on
 *  render, and only the owning tab re-renders when its model resolves. */
export function useShellSessionModel(tab: Tab | null | undefined): string | null {
  const backing = tab?.backing;
  const isShell = backing?.kind === "chat_backend";
  const pluginId = isShell ? backing.plugin_id : null;
  const sessionId = isShell ? backing.session_id : null;

  const model = useSyncExternalStore(
    subscribeShellModel,
    () => getCachedShellModel(pluginId, sessionId),
    () => null,
  );

  useEffect(() => {
    if (!pluginId || !sessionId) return;
    let alive = true;
    const refresh = () => {
      void fetchShellSessionModel(pluginId, sessionId);
    };
    refresh();
    let unlisten: (() => void) | undefined;
    void listen("mux-state-changed", refresh).then((un) => {
      if (alive) unlisten = un;
      else un();
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [pluginId, sessionId]);

  return model;
}
