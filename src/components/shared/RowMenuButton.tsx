import type { Ref } from "react";

export function KebabGlyph() {
  return (
    <svg className="ct-kebab-glyph" viewBox="0 0 4 14" aria-hidden="true">
      <circle cx="2" cy="2" r="1.3" />
      <circle cx="2" cy="7" r="1.3" />
      <circle cx="2" cy="12" r="1.3" />
    </svg>
  );
}

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
      className={`ct-row-kebab ${touchMode ? "ct-row-kebab--touch" : ""}`}
    >
      <KebabGlyph />
    </button>
  );
}
