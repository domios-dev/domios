import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../lib/bridge";

import type { ModelCatalogueChanged, ProviderDescriptor } from "../lib/types";
import { acceptModelCatalogueChange, listAgentDescriptors, resetAgentCatalogueVersions, setAgentDescriptors } from "../lib/agentDescriptorCache";
import { EVENT_STREAM_READY, eventStreamVersion, recordProviderCatalogueResponse } from "../lib/eventStreamReady";
import { shouldRefetch } from "../lib/providerCatalogueModel";
import { persistProviderIcons } from "../lib/providerIconCache";
import { createProviderCatalogueResource } from "../lib/providerCatalogueResource";

export function shouldRefreshAgentProviders(eventType: string): boolean {
  return eventType === "contributions-changed" || eventType === "model-catalogue-changed";
}

export function visibleAgentProviders(
  items: ProviderDescriptor[],
  _loading: boolean,
  _error: string | null,
): ProviderDescriptor[] {
  return items;
}

export function agentProvidersFromResponse(result: unknown): ProviderDescriptor[] {
  if (!Array.isArray(result)) throw new Error("Invalid agent provider catalog response");
  return result as ProviderDescriptor[];
}

export async function fetchAgentProviders(): Promise<ProviderDescriptor[]> {
  const result = await invoke<unknown>("list_agent_providers");
  const providers = agentProvidersFromResponse(result);
  recordProviderCatalogueResponse(providers.length);
  return providers;
}

const catalogueResource = createProviderCatalogueResource(fetchAgentProviders);
export const invalidateAgentProviders = catalogueResource.invalidate;

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
  const reset = () => observed.clear();
  if (typeof window !== "undefined") window.addEventListener(EVENT_STREAM_READY, reset);

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
    if (typeof window !== "undefined") window.removeEventListener(EVENT_STREAM_READY, reset);
    unlisten?.();
  };
}

export function useAgentProviders(): {
  providers: ProviderDescriptor[];
  loading: boolean;
  error: string | null;
  refetch: () => void;
} {
  const [items, setItems] = useState<ProviderDescriptor[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const queued = useRef(false);
  const active = useRef(false);
  const load = useCallback(() => {
    generation.current++;
    if (queued.current) return;
    queued.current = true;
    queueMicrotask(() => {
      queued.current = false;
      if (!active.current) return;
      const current = generation.current;
      setLoading(true);
      void catalogueResource.load().then((next) => {
        if (!active.current || generation.current !== current) return;
        persistProviderIcons(next);
        setItems(next);
        setError(null);
      }).catch((reason: unknown) => {
        if (active.current && generation.current === current) setError(String(reason));
      }).finally(() => {
        if (active.current && generation.current === current) setLoading(false);
      });
    });
  }, []);
  const refetch = useCallback(() => {
    invalidateAgentProviders();
    load();
  }, [load]);
  useEffect(() => {
    active.current = true;
    let disposed = false;
    let version: number | undefined;
    let unlisten: UnlistenFn | undefined;
    const ready = () => {
      const nextVersion = eventStreamVersion();
      if (shouldRefetch(version, nextVersion, true, null)) {
        version = nextVersion;
        resetAgentCatalogueVersions();
        refetch();
      }
    };
    const reset = () => {
      invalidateAgentProviders();
      generation.current++;
      resetAgentCatalogueVersions();
      setItems([]);
      setError(null);
      setLoading(true);
    };
    window.addEventListener(EVENT_STREAM_READY, ready);
    window.addEventListener("codeterm-peer-switch-reset", reset);
    window.addEventListener("codeterm-peer-switch-rebind", ready);
    const cleanupModels = attachModelCatalogueChangedRefresh(refetch);
    void listen<{ kind: string }>("contributions-changed", ({ payload }) => {
      if (!disposed && payload.kind === "agent_provider") refetch();
    }).then((dispose) => {
      if (disposed) { dispose(); return; }
      unlisten = dispose;
      // Fetch after registration so a missed boot event cannot leave a stale catalogue.
      version = eventStreamVersion();
      load();
    }).catch(() => { if (!disposed) load(); });
    return () => {
      disposed = true;
      active.current = false;
      generation.current++;
      unlisten?.();
      cleanupModels();
      window.removeEventListener(EVENT_STREAM_READY, ready);
      window.removeEventListener("codeterm-peer-switch-reset", reset);
      window.removeEventListener("codeterm-peer-switch-rebind", ready);
    };
  }, [refetch, load]);
  return { providers: visibleAgentProviders(items, loading, error), loading, error, refetch };
}

export function useAgentProviderCachePublisher(): void {
  const { providers, loading } = useAgentProviders();
  useLayoutEffect(() => {
    setAgentDescriptors(providers, loading);
  }, [providers, loading]);
}
