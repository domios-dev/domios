import { templateTitle, type TemplateSummary } from "../../lib/teamTemplates/startModel";
import { TemplateIcon } from "./TemplateIcon";
import { authorLine } from "../../lib/teamTemplates/exchangeModel";

const LINK = "text-[12px] text-[var(--ct-accent)] hover:underline";

export function TemplateHeader({ summary, onChange }: {
  summary: TemplateSummary;
  onChange: () => void;
}) {
  const author = authorLine(summary);
  return (
    <div className="ct-card ct-card--static flex items-start gap-3 p-3" data-testid="template-header">
      <TemplateIcon icon={summary.icon} />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span className="truncate text-[13px] font-medium text-[var(--ct-fg)]">{templateTitle(summary)}</span>
          {summary.version && <span className="font-mono text-[11px] text-[var(--ct-muted-fg)]">{summary.version}</span>}
        </div>
        <p className="mt-0.5 truncate text-[12px] leading-snug text-[var(--ct-muted-fg)]" title={summary.description}>{summary.description}</p>
        <div className="flex flex-wrap items-center gap-x-1 text-[12px] text-[var(--ct-muted-fg)]">
          {author && <span>{author}</span>}
          {author && <span aria-hidden="true">·</span>}
          <span className="inline-flex items-center gap-x-1 whitespace-nowrap" data-testid="template-header-links">
            <button type="button" className={LINK} onClick={onChange}>Change template</button>
          </span>
        </div>
      </div>
    </div>
  );
}
