import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../lib/bridge";
import { DEV_BUILD } from "../lib/devBuild";

import type { ModelCatalogueChanged, ProviderDescriptor } from "../lib/types";
import { acceptModelCatalogueChange, listAgentDescriptors, resetAgentCatalogueVersions, setAgentDescriptors } from "../lib/agentDescriptorCache";
import { recordProviderCatalogueResponse } from "../lib/eventStreamReady";
import { shouldRefetch, type ProviderCatalogueRevision } from "../lib/providerCatalogueModel";
import { persistProviderIcons } from "../lib/providerIconCache";
import { createProviderCatalogueResource } from "../lib/providerCatalogueResource";
import { WEB_BOOT_CHANGED_EVENT, type BootChangedDetail } from "../lib/buildDriftReload";
import { PROVIDER_AVAILABILITY_CHANGED } from "../lib/providerAvailability";
import { clearProviderProbes, probeProvider } from "../lib/providerAvailabilityProbe";

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
  const result = await invoke<unknown>("list_agent_providers", { metadataOnly: true });
  const providers = agentProvidersFromResponse(result);
  clearProviderProbes();
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
  scope?: { providerId: string; accountId?: string | null; contextKey?: string },
): () => void {
  let active = true;
  const observed = new Map<string, number>(scope ? [] : listAgentDescriptors().map((provider) => [
    provider.id,
    provider.catalogue_version ?? 0,
  ]));
  let unlisten: UnlistenFn | undefined;
  const reset = () => observed.clear();
  if (typeof window !== "undefined") {
    window.addEventListener("codeterm-peer-switch-reset", reset);
    window.addEventListener(WEB_BOOT_CHANGED_EVENT, reset);
  }

  void listenImpl<ModelCatalogueChanged>("model-catalogue-changed", (event) => {
    if (!active) return;
    const change = event.payload;
    const account = change.account_id ?? "default";
    if (scope) {
      if (change.provider !== scope.providerId || account !== (scope.accountId ?? "default")) return;
      if (scope.contextKey && change.context_key !== scope.contextKey) return;
    } else if (change.account_id != null || change.context_key != null) return;
    const key = scope ? JSON.stringify([change.provider, account, change.context_key]) : change.provider;
    if (change.version <= (observed.get(key) ?? 0)) return;
    observed.set(key, change.version);
    if (!scope) acceptModelCatalogueChange(change);
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
    if (typeof window !== "undefined") {
      window.removeEventListener("codeterm-peer-switch-reset", reset);
      window.removeEventListener(WEB_BOOT_CHANGED_EVENT, reset);
    }
    unlisten?.();
  };
}

export function useAgentProviders(peerId?: string): {
  providers: ProviderDescriptor[];
  loading: boolean;
  error: string | null;
  refetch: () => void;
} {
  const [items, setItems] = useState<ProviderDescriptor[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const hasLoaded = useRef(false);
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
      const read = peerId
        ? invoke<unknown>("list_agent_providers", { metadataOnly: true, peerId }).then(agentProvidersFromResponse)
        : catalogueResource.load();
      void read.then((next) => {
        if (!active.current || generation.current !== current) return;
        if (!peerId) persistProviderIcons(next);
        hasLoaded.current = true;
        setItems(next);
        setError(null);
        for (const provider of next.filter(provider => provider.availability_checking)) {
          void probeProvider(provider.id, peerId).then(status => {
            if (!active.current || generation.current !== current) return;
            setItems(items => items.map(item => item.id === provider.id ? {
              ...item,
              availability_checking: false,
              availability: status?.availability ?? "unknown",
              cli_status: status ?? undefined,
            } : item));
          });
        }
      }).catch((reason: unknown) => {
        if (!active.current || generation.current !== current) return;
        if (DEV_BUILD) console.debug("Background agent provider catalogue refresh failed", reason);
        if (!hasLoaded.current) setError(String(reason));
      }).finally(() => {
        if (active.current && generation.current === current) setLoading(false);
      });
    });
  }, [peerId]);
  const refetch = useCallback(() => {
    if (peerId) clearProviderProbes(peerId);
    else invalidateAgentProviders();
    load();
  }, [load, peerId]);
  useEffect(() => {
    active.current = true;
    hasLoaded.current = false;
    setItems([]);
    setError(null);
    setLoading(true);
    if (peerId) {
      load();
      return () => {
        active.current = false;
        generation.current++;
      };
    }
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    let serverIdentity: ProviderCatalogueRevision | undefined;
    const reset = () => {
      invalidateAgentProviders();
      generation.current++;
      resetAgentCatalogueVersions();
      clearProviderProbes();
      serverIdentity = undefined;
      hasLoaded.current = false;
      setItems([]);
      setError(null);
      setLoading(true);
    };
    const rebind = () => load();
    const serverChanged = (event: Event) => {
      const detail = (event as CustomEvent<BootChangedDetail>).detail;
      const next: ProviderCatalogueRevision = { boot: detail.boot, build: detail.build };
      if (!shouldRefetch(serverIdentity, next)) return;
      serverIdentity = next;
      resetAgentCatalogueVersions();
      refetch();
    };
    window.addEventListener("codeterm-peer-switch-reset", reset);
    window.addEventListener("codeterm-peer-switch-rebind", rebind);
    window.addEventListener(WEB_BOOT_CHANGED_EVENT, serverChanged);
    const cleanupModels = attachModelCatalogueChangedRefresh(refetch);
    let unlistenAvailability: UnlistenFn | undefined;
    void listen(PROVIDER_AVAILABILITY_CHANGED, () => { if (!disposed) refetch(); })
      .then((dispose) => { if (disposed) dispose(); else unlistenAvailability = dispose; })
      .catch(() => {});
    void listen<{ kind: string }>("contributions-changed", ({ payload }) => {
      if (!disposed && payload.kind === "agent_provider") refetch();
    }).then((dispose) => {
      if (disposed) { dispose(); return; }
      unlisten = dispose;
      // Establish the baseline after registration so contribution changes are not missed.
      load();
    }).catch(() => { if (!disposed) load(); });
    return () => {
      disposed = true;
      active.current = false;
      generation.current++;
      unlisten?.();
      unlistenAvailability?.();
      cleanupModels();
      window.removeEventListener("codeterm-peer-switch-reset", reset);
      window.removeEventListener("codeterm-peer-switch-rebind", rebind);
      window.removeEventListener(WEB_BOOT_CHANGED_EVENT, serverChanged);
    };
  }, [refetch, load, peerId]);
  return { providers: visibleAgentProviders(items, loading, error), loading, error, refetch };
}

export function useAgentProviderCachePublisher(): void {
  const { providers, loading } = useAgentProviders();
  useLayoutEffect(() => {
    setAgentDescriptors(providers, loading);
  }, [providers, loading]);
}
