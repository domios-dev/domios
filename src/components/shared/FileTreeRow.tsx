import type { ButtonHTMLAttributes, ReactNode } from "react";

export function FileTreeRow({ children, actions, selected, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { actions?: ReactNode; selected?: boolean }) {
  return <div className={`group/file-tree flex min-w-0 items-center rounded hover:bg-[var(--ct-hover)] focus-within:bg-[var(--ct-hover)] ${selected ? "bg-[var(--ct-hover)] text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)]"}`}>
    <button type="button" {...props} className="flex min-w-0 flex-1 items-center gap-1 rounded px-2 py-1 text-left text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]">{children}</button>
    {actions && <span className="flex shrink-0 gap-1 pr-1 invisible group-hover/file-tree:visible group-focus-within/file-tree:visible">{actions}</span>}
  </div>;
}
