import { useState, type ReactNode } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import type { OrchestrationPairValues } from "../Sidebar/OrchestrationPairForm";
import type { TemplateSummary } from "../../lib/teamTemplates/startModel";
import { TemplateHeader } from "./TemplateHeader";
import { TemplateGallery } from "./TemplateGallery";
import { TemplateReview } from "./TemplateReview";
import { TemplateInstructions } from "./TemplateInstructions";
import { TemplateStartFields, WorkerChips } from "./TemplateStartFields";
import { TemplateWhere } from "./TemplateWhere";
import { TemplateExportDialog } from "./TemplateExportDialog";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";
import { LaterManagerRows, type PairRoleControls } from "./TemplateManagers";

/** The Team panel of the session dialog: today's pair form under the default, or a chosen template's simple form. */
export function TeamTemplateSection({ choice, defaultForm, ...form }: {
  choice: TeamTemplateChoice;
  /** Today's pair form; `laterManagers` are the default's further roster rows to place after the Planner. */
  defaultForm: (laterManagers: ReactNode) => ReactNode;
  providers: ProviderDescriptor[];
  disabledProviderIds: ReadonlySet<string>;
  pair: OrchestrationPairValues;
  onPairChange: (next: OrchestrationPairValues) => void;
  onInstallProvider: (providerId: string) => void;
  roleControls?: PairRoleControls;
}) {
  const [exporting, setExporting] = useState<{ summary: TemplateSummary; share: boolean } | null>(null);
  const view = choice.view;
  const fallback = choice.defaultPreview;
  const body = view.kind === "gallery"
    ? <TemplateGallery choice={choice} onExport={(summary, share = false) => setExporting({ summary, share })} />
    : view.kind === "review"
      ? <TemplateReview choice={choice} preview={view.preview} mode={view.mode} providers={form.providers} onInstallProvider={form.onInstallProvider}
        onExport={() => setExporting({ summary: view.preview.form.template, share: false })} />
      : view.kind === "instructions"
        ? <TemplateInstructions preview={view.review.preview} role={view.role} onRole={choice.readInstructions} onBack={choice.backFromInstructions} />
        : choice.chosen
          ? <TemplateStartFields choice={choice} preview={choice.chosen} {...form} />
          : <div className="flex flex-col gap-3">
            <TemplateHeader summary={choice.defaultSummary} onChange={choice.openGallery} onDetails={fallback ? choice.showDetails : undefined} />
            <div>{defaultForm(fallback ? <LaterManagerRows form={fallback.form} providers={form.providers} disabledProviderIds={form.disabledProviderIds}
              pair={form.pair} seats={choice.seats} onSeatChange={choice.setSeat} onSeatClear={choice.clearSeat} onInstallProvider={form.onInstallProvider} /> : null)}</div>
            {fallback && <WorkerChips preview={fallback} />}
            {fallback && <TemplateWhere choice={choice} preview={fallback} providers={form.providers} onInstallProvider={form.onInstallProvider} />}
          </div>;
  const blocked = choice.blocked;
  return <>
    {blocked && <div role="alert" aria-label={`Can't start ${blocked.title}`} data-testid="template-blocked-start"
      className="mb-3 rounded-lg border border-[var(--ct-red)] px-3 py-2 text-[12px]">
      <div className="font-medium">{blocked.title}</div>
      <div className="text-[var(--ct-red)]">{blocked.refusal.message}</div>
      {blocked.refusal.reasons.map((reason) => <div key={reason} className="text-[var(--ct-muted-fg)]">{reason}</div>)}
    </div>}
    {body}
    {exporting && <TemplateExportDialog summary={exporting.summary} share={exporting.share} onClose={() => setExporting(null)} />}
  </>;
}
