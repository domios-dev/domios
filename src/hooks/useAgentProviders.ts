/**
 * Live list of agent providers contributed by active plugins.
 *
 * Single source of truth: the backend's `list_agent_providers` Tauri command,
 * which calls `dispatch::Command::ListAgentProviders` → `AgentRegistry::list_descriptors()`.
 * Auto-refetches when a `contributions-changed { kind: "agent_provider" }` event
 * arrives, and when the backend emits `models-updated` after refreshing live model catalogs.
 *
 * Adding a new bundled or third-party agent plugin requires ZERO changes here —
 * the descriptor flows through automatically.
 */
import { useCallback, useEffect } from "react";
import { invoke, listen, type UnlistenFn } from "../lib/bridge";

import type { ProviderDescriptor } from "../lib/types";
import { setAgentDescriptors } from "../lib/agentDescriptorCache";
import { useContributions } from "./useContributions";

export function shouldRefreshAgentProviders(eventType: string): boolean {
  return eventType === "contributions-changed" || eventType === "models-updated";
}

async function fetchAgentProviders(): Promise<ProviderDescriptor[]> {
  const result = await invoke<ProviderDescriptor[]>("list_agent_providers");
  return result ?? [];
}

// Avoid wiping the shared descriptor cache during a hook's initial loading beat.
export function syncAgentDescriptorCache(items: ProviderDescriptor[], loading: boolean): void {
  if (loading) return;
  setAgentDescriptors(items);
}

export function attachModelsUpdatedRefresh(
  refetch: () => void,
  listenImpl: typeof listen = listen,
): () => void {
  let active = true;
  let unlisten: UnlistenFn | undefined;

  void listenImpl<{ provider: string }>("models-updated", () => {
    if (active) refetch();
  }).then((fn) => {
    if (active) {
      unlisten = fn;
    } else {
      fn();
    }
  }).catch(() => {
    // Listener registration failed during reconnect; the next mount will retry.
  });

  return () => {
    active = false;
    unlisten?.();
  };
}

export function useAgentProviders(): {
  providers: ProviderDescriptor[];
  loading: boolean;
  error: string | null;
  refetch: () => void;
} {
  const fetcher = useCallback(fetchAgentProviders, []);
  const { items, loading, error, refetch } = useContributions<ProviderDescriptor>(
    "agent_provider",
    fetcher,
  );
  useEffect(() => {
    syncAgentDescriptorCache(items, loading);
  }, [items, loading]);
  useEffect(() => {
    return attachModelsUpdatedRefresh(refetch);
  }, [refetch]);
  return { providers: items, loading, error, refetch };
}
