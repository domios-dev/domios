import type { ReactNode, Ref } from "react";

export function ComposerCard({ children, header, footer, actions, ref }: { children: ReactNode; header?: ReactNode; footer?: ReactNode; actions?: ReactNode; ref?: Ref<HTMLDivElement> }) {
  return <div ref={ref} role="group" aria-label="Message composer" data-testid="composer-input-shell" className="ct-composer-card relative flex min-w-0 flex-1 flex-col focus-within:ring-2 focus-within:ring-[var(--ct-accent)]">
    {header && <div className="flex flex-wrap items-center gap-1 px-3 pt-2">{header}</div>}
    <div className="ct-composer-row relative flex min-w-0 items-center gap-2 py-0 pl-3.5 pr-0">{children}{actions}</div>
    {footer && <div className="flex flex-wrap items-center justify-between gap-2 px-3 pb-2">{footer}</div>}
  </div>;
}
