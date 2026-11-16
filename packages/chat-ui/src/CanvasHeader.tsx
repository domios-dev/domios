import type { ReactNode } from "react";
import { CANVAS_SEPARATOR } from "./CanvasToolbar";

export function CanvasHeader({ title, navigator, view, actions, close }: {
  title: ReactNode;
  navigator?: ReactNode;
  view: ReactNode;
  actions?: ReactNode;
  close: ReactNode;
}) {
  return <header className="@container flex h-[48px] min-h-[48px] shrink-0 flex-nowrap items-center gap-1 @max-[560px]:gap-0 border-b border-[var(--ct-border)] px-2 text-xs text-[var(--ct-muted-fg)]">
    <div className="flex min-w-0 shrink items-center gap-2">{title}</div>
    {navigator}
    <div className="min-w-0 flex-1 shrink-0" />
    {view}
    <span aria-hidden className={CANVAS_SEPARATOR} />
    {actions}
    {close}
  </header>;
}
