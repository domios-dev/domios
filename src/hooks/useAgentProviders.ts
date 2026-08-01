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
import { useCallback, useEffect, useLayoutEffect } from "react";
import { invoke, listen, type UnlistenFn } from "../lib/bridge";

import type { ProviderDescriptor } from "../lib/types";
import { setAgentDescriptors } from "../lib/agentDescriptorCache";
import { useContributions } from "./useContributions";

const EMPTY_AGENT_PROVIDERS: ProviderDescriptor[] = [];

export function shouldRefreshAgentProviders(eventType: string): boolean {
  return eventType === "contributions-changed" || eventType === "models-updated";
}

export function visibleAgentProviders(
  items: ProviderDescriptor[],
  loading: boolean,
  error: string | null,
): ProviderDescriptor[] {
  return loading || error ? EMPTY_AGENT_PROVIDERS : items;
}

export function agentProvidersFromResponse(result: unknown): ProviderDescriptor[] {
  return Array.isArray(result) ? (result as ProviderDescriptor[]) : [];
}

async function fetchAgentProviders(): Promise<ProviderDescriptor[]> {
  const result = await invoke<unknown>("list_agent_providers");
  return agentProvidersFromResponse(result);
}

export function syncAgentDescriptorCache(
  items: ProviderDescriptor[],
  loading: boolean,
  error: string | null,
): void {
  setAgentDescriptors(visibleAgentProviders(items, loading, error));
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
    return attachModelsUpdatedRefresh(refetch);
  }, [refetch]);
  return { providers: visibleAgentProviders(items, loading, error), loading, error, refetch };
}

export function useAgentProviderCachePublisher(): void {
  const { providers } = useAgentProviders();
  useLayoutEffect(() => {
    setAgentDescriptors(providers);
  }, [providers]);
}
