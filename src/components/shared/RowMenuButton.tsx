import type { Ref } from "react";

export function RowMenuButton({ label, title, touchMode = false, ref }: {
  label: string;
  title?: string;
  touchMode?: boolean;
  ref?: Ref<HTMLButtonElement>;
}) {
  return (
    <button
      ref={ref}
      type="button"
      aria-label={label}
      aria-haspopup="menu"
      title={title ?? label}
      className={`${touchMode ? "h-11 w-11" : "w-5 h-5"} flex items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-border)] hover:text-[var(--ct-fg)] transition-colors focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--ct-accent)]`}
    >
      <span aria-hidden="true" className={`${touchMode ? "text-[20px]" : "text-[13px]"} leading-none font-bold`}>⋮</span>
    </button>
  );
}
