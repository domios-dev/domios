import { ExpandableReader } from "./ExpandableReader";
import type { CSSProperties, ReactNode } from "react";
import { CardChevron } from "./CompactCard";

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
  /** Stays in the header whether or not the body is expanded. */
  status?: ReactNode;
  children: ReactNode;
}

/** Collapsible inline notice in the transcript, on the shared ct-card surface. */
export function ChatNoticeCard({ testId, icon, title, subtitle, tone, expanded, onToggle, bodyClassName = "", role, status, children }: Props) {
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
        {status}
        {opener}
        <CardChevron expanded={expanded} label={expanded ? "Collapse notice" : "Expand notice"} onToggle={onToggle} className="ct-reader-opener ct-notice-chevron" style={{}} />
        </div>
        {(expanded || fullscreen) && <div className={`ct-card__body ct-card__body--reveal ${bodyClassName}`}>{children}</div>}
      </div>
    </div>
    )}</ExpandableReader>
  );
}
