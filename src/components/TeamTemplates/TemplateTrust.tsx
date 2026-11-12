import { templateSource, type TemplateSummary } from "../../lib/teamTemplates/startModel";

/**
 * Where a package came from, one element everywhere: an amber dot and "From file · unverified" until its first Use,
 * then a plain chip ("Mine", "From Mira", "Built-in").
 */
export function TemplateTrust({ summary }: { summary: Pick<TemplateSummary, "provenance" | "received_from" | "verified"> }) {
  const source = templateSource(summary);
  if (source.unverified) {
    return (
      <span className="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap text-[11px] text-[var(--ct-muted-fg)]" data-testid="template-source" data-unverified="true">
        <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-[var(--ct-yellow,#d97706)]" />
        {source.label}
      </span>
    );
  }
  return <span className="ct-card__chip" data-testid="template-source">{source.label}</span>;
}
