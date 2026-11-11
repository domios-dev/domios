import { useState, type ReactNode } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import type { OrchestrationPairValues } from "../Sidebar/OrchestrationPairForm";
import type { TemplateSummary } from "../../lib/teamTemplates/startModel";
import { TemplateHeader } from "./TemplateHeader";
import { TemplateGallery } from "./TemplateGallery";
import { TemplateReview } from "./TemplateReview";
import { TemplateStartFields } from "./TemplateStartFields";
import { TemplateExportDialog } from "./TemplateExportDialog";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";
import type { PairRoleControls } from "./TemplateManagers";

/** The Team panel of the session dialog: today's pair form under the default, or a chosen template's simple form. */
export function TeamTemplateSection({ choice, defaultForm, ...form }: {
  choice: TeamTemplateChoice;
  defaultForm: ReactNode;
  providers: ProviderDescriptor[];
  disabledProviderIds: ReadonlySet<string>;
  pair: OrchestrationPairValues;
  onPairChange: (next: OrchestrationPairValues) => void;
  onInstallProvider: (providerId: string) => void;
  roleControls?: PairRoleControls;
}) {
  const [exporting, setExporting] = useState<{ summary: TemplateSummary; share: boolean } | null>(null);
  const view = choice.view;
  const body = view.kind === "gallery"
    ? <TemplateGallery choice={choice} onExport={(summary, share = false) => setExporting({ summary, share })} />
    : view.kind === "review"
      ? <TemplateReview choice={choice} preview={view.preview} mode={view.mode} onInstallProvider={form.onInstallProvider}
        onExport={() => setExporting({ summary: view.preview.form.template, share: false })} />
      : choice.chosen
        ? <TemplateStartFields choice={choice} preview={choice.chosen} {...form} />
        : <div className="flex flex-col gap-3">
          <TemplateHeader summary={choice.defaultSummary} onChange={choice.openGallery} />
          <div>{defaultForm}</div>
        </div>;
  return <>
    {body}
    {exporting && <TemplateExportDialog summary={exporting.summary} share={exporting.share} onClose={() => setExporting(null)} />}
  </>;
}
