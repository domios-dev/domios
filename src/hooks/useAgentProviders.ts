/**
 * Live list of agent providers contributed by active plugins.
 *
 * Single source of truth: the backend's `list_agent_providers` Tauri command,
 * which returns the transport-tagged ProviderRegistry snapshot.
 * Auto-refetches when provider contributions or a versioned model catalogue changes.
 *
 * Adding a new bundled or third-party agent plugin requires ZERO changes here —
 * the descriptor flows through automatically.
 */
import { useCallback, useEffect, useLayoutEffect } from "react";
import { invoke, listen, type UnlistenFn } from "../lib/bridge";

import type { ModelCatalogueChanged, ProviderDescriptor } from "../lib/types";
import { acceptModelCatalogueChange, listAgentDescriptors, setAgentDescriptors } from "../lib/agentDescriptorCache";
import { useContributions } from "./useContributions";

const EMPTY_AGENT_PROVIDERS: ProviderDescriptor[] = [];

export function shouldRefreshAgentProviders(eventType: string): boolean {
  return eventType === "contributions-changed" || eventType === "model-catalogue-changed";
}

export function visibleAgentProviders(
  items: ProviderDescriptor[],
  _loading: boolean,
  error: string | null,
): ProviderDescriptor[] {
  return error ? EMPTY_AGENT_PROVIDERS : items;
}

export function agentProvidersFromResponse(result: unknown): ProviderDescriptor[] {
  return Array.isArray(result) ? (result as ProviderDescriptor[]) : [];
}

export async function fetchAgentProviders(): Promise<ProviderDescriptor[]> {
  const result = await invoke<unknown>("list_agent_providers");
  return agentProvidersFromResponse(result);
}

export function syncAgentDescriptorCache(
  items: ProviderDescriptor[],
  loading: boolean,
  error: string | null,
): void {
  setAgentDescriptors(visibleAgentProviders(items, loading, error), loading);
}

export function attachModelCatalogueChangedRefresh(
  refetch: () => void,
  listenImpl: typeof listen = listen,
): () => void {
  let active = true;
  const observed = new Map(listAgentDescriptors().map((provider) => [provider.id, provider.catalogue_version ?? 0]));
  let unlisten: UnlistenFn | undefined;

  void listenImpl<ModelCatalogueChanged>("model-catalogue-changed", (event) => {
    if (!active || event.payload.version <= (observed.get(event.payload.provider) ?? 0)) return;
    observed.set(event.payload.provider, event.payload.version);
    acceptModelCatalogueChange(event.payload);
    refetch();
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
    return attachModelCatalogueChangedRefresh(refetch);
  }, [refetch]);
  return { providers: visibleAgentProviders(items, loading, error), loading, error, refetch };
}

export function useAgentProviderCachePublisher(): void {
  const { providers, loading } = useAgentProviders();
  useLayoutEffect(() => {
    setAgentDescriptors(providers, loading);
  }, [providers, loading]);
}
