import { templateSource, type TemplateOrigin } from "../../lib/teamTemplates/startModel";
import { useTeamChannelLabel } from "../../lib/teamTemplates/teamChannels";

const DOT = "h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--ct-yellow,#d97706)]";

/**
 * Where a package came from, one element everywhere: an amber dot and "From file · unverified" until its first Use,
 * then a plain chip ("Mine", "From Mira", "From Acme teams", "Built-in"). `inline` is the same words as muted meta text, for a narrow card.
 */
export function TemplateTrust({ summary, inline = false }: { summary: TemplateOrigin; inline?: boolean }) {
  const channelLabel = useTeamChannelLabel(summary.provenance === "channel" ? summary.channel : null);
  const source = templateSource(summary, channelLabel);
  if (inline) {
    return (
      <span className="inline-flex min-w-0 max-w-full items-center gap-1 whitespace-nowrap" title={source.label} data-unverified={source.unverified || undefined}>
        {source.unverified && <span aria-hidden="true" className={DOT} />}
        <span className="template-trust-label">{source.label}</span>
      </span>
    );
  }
  if (source.unverified) {
    return (
      <span className="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap text-[11px] text-[var(--ct-muted-fg)]" data-testid="template-source" data-unverified="true">
        <span aria-hidden="true" className={DOT} />
        {source.label}
      </span>
    );
  }
  return <span className="ct-card__chip" data-testid="template-source">{source.label}</span>;
}
