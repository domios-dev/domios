import type { ReactNode } from "react";

/** The one presentation of an owner decision: what asks, what it means, and the choices. It records nothing. */
export function ConsentLayout({ titleId, descriptionId, title, description, footnote, actions, children }: {
  titleId: string;
  descriptionId: string;
  title: ReactNode;
  description: ReactNode;
  footnote?: ReactNode;
  actions: ReactNode;
  children?: ReactNode;
}) {
  return (
    <>
      <h2 id={titleId} className="text-base font-semibold">{title}</h2>
      <p id={descriptionId} className="text-sm text-[var(--ct-muted)]">{description}</p>
      {children}
      <div className="flex flex-wrap items-center justify-end gap-2">
        {footnote && <div className="mr-auto min-w-0 text-[12px] text-[var(--ct-muted-fg)]">{footnote}</div>}
        {actions}
      </div>
    </>
  );
}
