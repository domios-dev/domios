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
    <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto" role={role}>
      <div data-testid={testId} className="ct-card" style={tone ? ({ "--ct-card-tone": tone } as CSSProperties) : undefined}>
        <button type="button" aria-expanded={expanded} onClick={onToggle} className="ct-card__head">
          <span className="ct-card__icon">{icon}</span>
          <span className="min-w-0 flex-1">
            <span className="ct-card__title truncate">{title}</span>
            {subtitle && <span className="ct-card__meta block truncate">{subtitle}</span>}
          </span>
          <ChevronIcon expanded={expanded} className="opacity-60" />
        </button>
        {expanded && <div className={`ct-card__body ct-card__body--reveal ${bodyClassName}`}>{children}</div>}
      </div>
    </div>
  );
}
