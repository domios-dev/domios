import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { DEV_BUILD } from "../../lib/devBuild";
import { BUILTIN_PLAN_CATALOG, builtinCatalogSource, type PlanCatalog, type PlanCatalogSource } from "./planCatalog";

const SourceContext = createContext<PlanCatalogSource>(builtinCatalogSource);

/** Swaps where every plan surface below reads offers and prices from. */
export function PlanCatalogProvider({ source, children }: { source: PlanCatalogSource; children: ReactNode }) {
  return <SourceContext.Provider value={source}>{children}</SourceContext.Provider>;
}

/** The current catalog; the built-in table stays in place until the source answers or if it fails. */
export function usePlanCatalog(): PlanCatalog {
  const source = useContext(SourceContext);
  const [catalog, setCatalog] = useState<PlanCatalog>(BUILTIN_PLAN_CATALOG);
  useEffect(() => {
    const controller = new AbortController();
    source.load(controller.signal).then((loaded) => {
      if (!controller.signal.aborted) setCatalog(loaded);
    }).catch((error: unknown) => {
      if (!controller.signal.aborted && DEV_BUILD) console.debug("Plan catalog load failed", error);
    });
    return () => controller.abort();
  }, [source]);
  return catalog;
}
