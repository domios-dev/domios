import { templateTitle, type TemplateSummary } from "../../lib/teamTemplates/startModel";
import { TemplateIcon } from "./TemplateIcon";
import { authorLine } from "../../lib/teamTemplates/exchangeModel";

const LINK = "text-[12px] text-[var(--ct-accent)] hover:underline";

export function TemplateHeader({ summary, onChange, onDetails }: {
  summary: TemplateSummary;
  onChange: () => void;
  onDetails?: () => void;
}) {
  return (
    <div className="ct-card ct-card--static flex items-start gap-3 p-3" data-testid="template-header">
      <TemplateIcon icon={summary.icon} />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span className="truncate text-[13px] font-medium text-[var(--ct-fg)]">{templateTitle(summary)}</span>
          <span className="font-mono text-[11px] text-[var(--ct-muted-fg)]">{summary.version}</span>
        </div>
        <p className="mt-0.5 text-[12px] leading-snug text-[var(--ct-muted-fg)]">{summary.description}</p>
        {(authorLine(summary) || onDetails) && <div className="text-[12px] text-[var(--ct-muted-fg)]">
          {authorLine(summary)}{authorLine(summary) && onDetails && " · "}
          {onDetails && <button type="button" className={LINK} onClick={onDetails}>Details</button>}
        </div>}
      </div>
      <button type="button" className={`${LINK} shrink-0`} onClick={onChange}>Change template</button>
    </div>
  );
}
