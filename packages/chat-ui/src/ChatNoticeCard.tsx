import { ExpandableReader } from "./ExpandableReader";
import type { CSSProperties, ReactNode } from "react";
import { ChevronIcon } from "./CodeBlock";

interface Props {
  testId: string;
  icon: ReactNode;
  title: ReactNode;
  subtitle?: ReactNode;
  /** CSS color for the icon tile and the hover tint. */
  tone?: string;
  expanded: boolean;
  onToggle: () => void;
  bodyClassName?: string;
  role?: string;
  children: ReactNode;
}

/** Collapsible inline notice in the transcript, on the shared ct-card surface. */
export function ChatNoticeCard({ testId, icon, title, subtitle, tone, expanded, onToggle, bodyClassName = "", role, children }: Props) {
  return (
    <ExpandableReader label={typeof title === "string" ? title : "Notice"} enabled={true} openerPlacement="header">{(fullscreen, opener) => (

    <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto" role={role}>
      <div data-testid={testId} className="ct-card" style={tone ? ({ "--ct-card-tone": tone } as CSSProperties) : undefined}>
        <div className="ct-card__head ct-reader-row">
        <button type="button" aria-expanded={expanded} onClick={onToggle} className="flex min-w-0 flex-1 items-center gap-2.5 text-left">
          <span className="ct-card__icon">{icon}</span>
          <span className="min-w-0 flex-1">
            <span className="ct-card__title truncate">{title}</span>
            {subtitle && <span className="ct-card__meta block truncate">{subtitle}</span>}
          </span>
        </button>
        {opener}
        <button type="button" aria-label={expanded ? "Collapse notice" : "Expand notice"} aria-expanded={expanded} onClick={onToggle} className="flex shrink-0 items-center">
          <ChevronIcon expanded={expanded} className="opacity-60" />
        </button>
        </div>
        {(expanded || fullscreen) && <div className={`ct-card__body ct-card__body--reveal ${bodyClassName}`}>{children}</div>}
      </div>
    </div>
    )}</ExpandableReader>
  );
}
