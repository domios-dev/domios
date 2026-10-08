import { useEffect, useState } from "react";
import { daemonFetch, type DaemonApiConfig } from "../../lib/webApi";
import { rememberDeclaredSurfaces } from "./surfaceIcon";
import { findDeclaredSurface, parseDeclaredSurfaces, type SurfaceDeclLookup } from "./surfaceRenderPlan";

const LOADING: SurfaceDeclLookup = { status: "loading" };

export function useSurfaceDecl(api: DaemonApiConfig | undefined, pluginId: string, surfaceId: string): SurfaceDeclLookup {
  const [lookup, setLookup] = useState<SurfaceDeclLookup>(LOADING);
  const host = api?.host;
  const port = api?.port;
  const token = api?.token;

  useEffect(() => {
    const controller = new AbortController();
    setLookup(LOADING);
    daemonFetch({ host, port, token }, "/api/surfaces", { signal: controller.signal })
      .then(async (response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const declared = parseDeclaredSurfaces(await response.json());
        rememberDeclaredSurfaces(declared);
        if (!controller.signal.aborted) setLookup({ status: "ready", declared: findDeclaredSurface(declared, pluginId, surfaceId) });
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) setLookup({ status: "failed", error: error instanceof Error ? error.message : String(error) });
      });
    return () => controller.abort();
  }, [host, port, token, pluginId, surfaceId]);

  return lookup;
}
