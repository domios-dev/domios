import { ExpandableReader } from "./ExpandableReader";
import { useId, type CSSProperties, type ReactNode } from "react";
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
  quiet?: boolean;
  detailsLabel?: string;
}

/** Collapsible inline notice in the transcript, on the shared ct-card surface. */
export function ChatNoticeCard({ testId, icon, title, subtitle, tone, expanded, onToggle, bodyClassName = "", role, status, children, quiet = false, detailsLabel }: Props) {
  const detailsId = useId();
  const iconChip = <span className="ct-card__icon size-[24px]!">{icon}</span>;
  return (
    <ExpandableReader label={typeof title === "string" ? title : "Notice"} enabled={!detailsLabel} openerPlacement="header">{(fullscreen, opener) => (

    <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto" role={detailsLabel ? undefined : role} aria-live={detailsLabel ? "off" : undefined}>
      <div data-testid={testId} className={`ct-card${detailsLabel ? " ct-card--static" : quiet ? " ct-card--quiet" : ""}`} style={tone ? ({ "--ct-card-tone": tone } as CSSProperties) : undefined}>
        <div className="ct-card__head ct-reader-row">
        {detailsLabel ? <div className="flex min-w-0 flex-1 items-center gap-2.5 text-left">
          {iconChip}
          <span role={role} className="text-[13px]">{title}</span>
        </div> : <button type="button" aria-expanded={expanded} onClick={onToggle} className="flex min-w-0 flex-1 items-center gap-2.5 text-left">
          {iconChip}
          <span className="min-w-0 flex-1">
            <span className="ct-card__title truncate">{title}</span>
            {subtitle && <span className="ct-card__meta block truncate">{subtitle}</span>}
          </span>
        </button>}
        {status}
        {opener}
        {!detailsLabel && <CardChevron expanded={expanded} label={expanded ? "Collapse notice" : "Expand notice"} onToggle={onToggle} className="ct-reader-opener ct-notice-chevron" style={{}} />}
        </div>
        {detailsLabel && children && <CardChevron expanded={expanded} label={detailsLabel} controls={detailsId} onToggle={onToggle}
          className="mx-3 mb-2 inline-flex items-center gap-1 rounded text-xs hover:brightness-125">{detailsLabel}</CardChevron>}
        {detailsLabel ? children && <div id={detailsId} hidden={!expanded} className={`ct-card__body ct-card__body--reveal ${bodyClassName}`}>
          {expanded && children}
        </div> : (expanded || fullscreen) && <div className={`ct-card__body ct-card__body--reveal ${bodyClassName}`}>{children}</div>}
      </div>
    </div>
    )}</ExpandableReader>
  );
}
