import { useEffect, useState } from "react";
import type { TeamTemplatesTransport } from "../../lib/transport/teamTemplates";
import { galleryEntries, startErrorView, startRefusalView, type StartErrorView, type TemplateSummary } from "../../lib/teamTemplates/startModel";
import type { TemplateCatalog } from "./useTeamTemplateChoice";

export type TemplateCatalogStatus = { kind: "ready" } | { kind: "refused" | "unavailable"; reason: StartErrorView };
export const templateCatalogKey = (summary: TemplateSummary) => JSON.stringify([summary.name, summary.version]);

export function useTemplateCatalogRefusals(enabled: boolean, catalog: TemplateCatalog | null, fallback: TemplateSummary, transport: () => TeamTemplatesTransport): Record<string, TemplateCatalogStatus> {
  const [snapshot, setSnapshot] = useState<{ catalog: TemplateCatalog; statuses: Record<string, TemplateCatalogStatus> } | null>(null);
  useEffect(() => {
    setSnapshot(null);
    if (!enabled || !catalog) return;
    const controller = new AbortController();
    void (async () => {
      for (const summary of galleryEntries(catalog.templates, catalog.recall, fallback, "all", "")) {
        if (controller.signal.aborted) return;
        let status: TemplateCatalogStatus;
        try {
          await transport().preview({ name: summary.name, version: summary.version || null }, { signal: controller.signal });
          status = { kind: "ready" };
        } catch (error) {
          const refusal = startRefusalView(error);
          status = refusal ? { kind: "refused", reason: refusal } : { kind: "unavailable", reason: startErrorView(error) };
        }
        if (controller.signal.aborted) return;
        setSnapshot((current) => ({ catalog, statuses: { ...current?.statuses, [templateCatalogKey(summary)]: status } }));
      }
    })();
    return () => controller.abort();
  }, [enabled, catalog, fallback, transport]);

  return enabled && snapshot?.catalog === catalog ? snapshot.statuses : {};
}
