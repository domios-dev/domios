import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "../../lib/bridge";
import { daemonFetch, type DaemonApiConfig } from "../../lib/webApi";
import { rememberDeclaredSurfaces } from "./surfaceIcon";
import { findDeclaredSurface, parseDeclaredSurfaces, type SurfaceDeclLookup } from "./surfaceRenderPlan";

const LOADING: SurfaceDeclLookup = { status: "loading" };

// A transient refetch failure must not tear down a surface that is already rendering.
export function nextSurfaceDeclLookup(previous: SurfaceDeclLookup, next: SurfaceDeclLookup): SurfaceDeclLookup {
  if (next.status === "failed" && previous.status === "ready" && previous.declared) return previous;
  return next;
}

function usePluginsGeneration(): number | null {
  const [generation, setGeneration] = useState<number | null>(null);
  useEffect(() => {
    let alive = true;
    let unlisten: UnlistenFn | undefined;
    listen("contributions-changed", () => {
      if (alive) setGeneration((n) => (n ?? 0) + 1);
    })
      .then((fn) => {
        if (alive) {
          unlisten = fn;
          setGeneration((current) => current ?? 0);
        } else fn();
      })
      .catch(() => {
        if (alive) setGeneration(0);
      });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return generation;
}

export function useSurfaceDecl(api: DaemonApiConfig | undefined, pluginId: string, surfaceId: string, refreshGeneration = 0): SurfaceDeclLookup {
  const [lookup, setLookup] = useState<SurfaceDeclLookup>(LOADING);
  const pluginsGeneration = usePluginsGeneration();
  const identity = useRef<string | null>(null);
  const host = api?.host;
  const port = api?.port;
  const token = api?.token;

  useEffect(() => {
    if (pluginsGeneration === null) return;
    const controller = new AbortController();
    const key = JSON.stringify([host, port, token, pluginId, surfaceId]);
    if (identity.current !== key) {
      identity.current = key;
      setLookup(LOADING);
    }
    const settle = (next: SurfaceDeclLookup) => {
      if (controller.signal.aborted) return;
      setLookup((previous) => nextSurfaceDeclLookup(previous, next));
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
    };
  }, [host, port, token, pluginId, surfaceId, pluginsGeneration, refreshGeneration]);

  return lookup;
}
