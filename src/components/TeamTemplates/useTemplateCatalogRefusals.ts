import { useEffect, useState } from "react";
import type { TeamTemplatesTransport } from "../../lib/transport/teamTemplates";
import { galleryEntries, startRefusalView, templateTitle, type TemplateSummary } from "../../lib/teamTemplates/startModel";
import type { BlockedTemplate, TemplateCatalog } from "./useTeamTemplateChoice";

export function useTemplateCatalogRefusals(enabled: boolean, catalog: TemplateCatalog | null, fallback: TemplateSummary, transport: () => TeamTemplatesTransport) {
  const [catalogRefusals, setCatalogRefusals] = useState<BlockedTemplate[]>([]);
  useEffect(() => {
    if (!enabled || !catalog) return;
    const controller = new AbortController();
    setCatalogRefusals([]);
    void (async () => {
      for (const summary of galleryEntries(catalog.templates, catalog.recall, fallback, "all", "")) {
        if (controller.signal.aborted) return;
        try {
          await transport().preview({ name: summary.name, version: summary.version || null }, { signal: controller.signal });
        } catch (error) {
          const refusal = startRefusalView(error);
          if (!controller.signal.aborted && refusal) {
            setCatalogRefusals((current) => [...current, { name: summary.name, version: summary.version, title: templateTitle(summary), refusal }]);
          }
        }
      }
    })();
    return () => controller.abort();
  }, [enabled, catalog, fallback, transport]);

  return catalogRefusals;
}
