import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "../../lib/bridge";
import { daemonFetch, type DaemonApiConfig } from "../../lib/webApi";
import { rememberDeclaredSurfaces } from "./surfaceIcon";
import { findDeclaredSurface, parseDeclaredSurfaces, type SurfaceDeclLookup } from "./surfaceRenderPlan";

const LOADING: SurfaceDeclLookup = { status: "loading" };
const RETRY_DELAYS_MS = [1_000, 2_000, 5_000];
const RETRY_STEADY_MS = 15_000;

export function surfaceDeclRetryDelay(attempt: number): number {
  return RETRY_DELAYS_MS[attempt] ?? RETRY_STEADY_MS;
}

// A transient refetch failure must not tear down a surface that is already rendering.
export function nextSurfaceDeclLookup(previous: SurfaceDeclLookup, next: SurfaceDeclLookup): SurfaceDeclLookup {
  if (next.status === "failed" && previous.status === "ready" && previous.declared) return previous;
  return next;
}

function usePluginsGeneration(): number {
  const [generation, setGeneration] = useState(0);
  useEffect(() => {
    let alive = true;
    let unlisten: UnlistenFn | undefined;
    listen("contributions-changed", () => {
      if (alive) setGeneration((n) => n + 1);
    })
      .then((fn) => {
        if (alive) unlisten = fn;
        else fn();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return generation;
}

export function useSurfaceDecl(api: DaemonApiConfig | undefined, pluginId: string, surfaceId: string): SurfaceDeclLookup {
  const [lookup, setLookup] = useState<SurfaceDeclLookup>(LOADING);
  const pluginsGeneration = usePluginsGeneration();
  const identity = useRef<string | null>(null);
  const host = api?.host;
  const port = api?.port;
  const token = api?.token;

  useEffect(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let attempt = 0;
    const key = JSON.stringify([host, port, token, pluginId, surfaceId]);
    if (identity.current !== key) {
      identity.current = key;
      setLookup(LOADING);
    }
    const settle = (next: SurfaceDeclLookup) => {
      if (controller.signal.aborted) return;
      setLookup((previous) => nextSurfaceDeclLookup(previous, next));
      if (next.status !== "ready" || !next.declared) timer = setTimeout(load, surfaceDeclRetryDelay(attempt++));
    };
    const load = () => {
      daemonFetch({ host, port, token }, "/api/surfaces", { signal: controller.signal })
        .then(async (response) => {
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          const declared = parseDeclaredSurfaces(await response.json());
          rememberDeclaredSurfaces(declared);
          settle({ status: "ready", declared: findDeclaredSurface(declared, pluginId, surfaceId) });
        })
        .catch((error: unknown) => settle({ status: "failed", error: error instanceof Error ? error.message : String(error) }));
    };
    load();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [host, port, token, pluginId, surfaceId, pluginsGeneration]);

  return lookup;
}
