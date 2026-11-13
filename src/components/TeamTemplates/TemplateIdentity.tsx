import type { ReactNode } from "react";
import { BookOpen, Bug, GitBranch, Home } from "lucide-react";
import { humanizePackageName, templateTitle, type TemplateSummary } from "../../lib/teamTemplates/startModel";
import { authorRows, linkRows } from "../../lib/teamTemplates/exchangeModel";
import { TemplateIcon } from "./TemplateIcon";
import { TemplateTrust } from "./TemplateTrust";
const LINK = "inline-flex items-center gap-1 text-[var(--ct-accent)] hover:underline";
const LINK_ICON: Record<string, ReactNode> = {
  Homepage: <Home size={12} aria-hidden="true" />, Repository: <GitBranch size={12} aria-hidden="true" />,
  Issues: <Bug size={12} aria-hidden="true" />, Docs: <BookOpen size={12} aria-hidden="true" />,
};

export function TemplateIdentity({ summary, titleId, descriptionId }: { summary: TemplateSummary; titleId?: string; descriptionId?: string }) {
  const authors = authorRows(summary);
  const links = linkRows(summary);
  const lineage = summary.derived_from ? `based on ${humanizePackageName(summary.derived_from.name)} ${summary.derived_from.version}` : null;
  const facts = [summary.license, lineage].filter(Boolean).join(" · ");
  return (
    <div className="flex gap-3.5" data-testid="template-identity">
      <TemplateIcon icon={summary.icon} size={44} />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
          <h2 id={titleId} className="text-[15px] font-semibold">{templateTitle(summary)}</h2>
          {summary.version && <span className="font-mono text-[11px] text-[var(--ct-muted-fg)]">{summary.version}</span>}
          <TemplateTrust summary={summary} />
        </div>
        <p id={descriptionId} className="mt-0.5 text-[12.5px] leading-snug text-[color-mix(in_srgb,var(--ct-fg)_85%,transparent)]">{summary.description}</p>
        {(authors.length > 0 || facts) && <div className="mt-1.5 flex flex-wrap gap-x-3.5 gap-y-1 text-[11.5px] text-[var(--ct-muted-fg)]">
          {authors.map((author, index) => <span key={`${author.name}:${index}`}>
            {author.url ? <a href={author.url} target="_blank" rel="noreferrer noopener" className={LINK}>{author.name}</a> : author.name}
            {author.role && ` · ${author.role}`}
          </span>)}
          {facts && <span>{facts}</span>}
        </div>}
        {links.length > 0 && <div className="mt-1 flex flex-wrap gap-x-3.5 gap-y-1 text-[11.5px]">{links.map((link) =>
          <a key={link.label} href={link.url} target="_blank" rel="noreferrer noopener" className={LINK}>{LINK_ICON[link.label]}{link.label}</a>)}</div>}
      </div>
    </div>
  );
}
