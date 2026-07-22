import type { ReactNode } from "react";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
  title?: string;
}

interface SegmentedProps<T extends string> {
  value: T;
  options: SegmentedOption<T>[];
  onChange: (value: T) => void;
  /** Track height in px — every segment and the thumb share it. */
  height?: number;
  className?: string;
  "data-testid"?: string;
  "data-pane-id"?: string;
  "aria-label"?: string;
}

/**
 * Two-or-more-way pill switch with a sliding thumb. Theme-driven (derives every
 * color from --ct tokens so it reads on any palette). Uses an equal-column grid
 * (`grid-auto-columns: 1fr`) so every segment is the SAME width regardless of
 * label length — flex `flex-1` would let the longest label claim more than its
 * share (min-content), throwing the half-width thumb off-centre.
 */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  height = 24,
  className = "",
  ...rest
}: SegmentedProps<T>) {
  const count = options.length;
  const activeIndex = Math.max(0, options.findIndex((o) => o.value === value));
  return (
    <div
      role="tablist"
      data-testid={rest["data-testid"]}
      data-pane-id={rest["data-pane-id"]}
      aria-label={rest["aria-label"]}
      className={`relative inline-grid select-none rounded-full p-[2px] ${className}`}
      style={{
        height,
        gridAutoFlow: "column",
        gridAutoColumns: "1fr",
        background: "var(--ct-bg)",
        border: "1px solid var(--ct-border)",
      }}
    >
      <span
        aria-hidden
        className="absolute rounded-full transition-[left] duration-200 ease-out"
        style={{
          top: 2,
          bottom: 2,
          // Same basis as the equal grid columns (containing-block padding box).
          left: `calc(2px + ${activeIndex} * (100% - 4px) / ${count})`,
          width: `calc((100% - 4px) / ${count})`,
          background: "var(--ct-accent)",
        }}
      />
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="tab"
            data-value={o.value}
            aria-selected={active}
            title={o.title ?? o.label}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => onChange(o.value)}
            className={`relative z-[1] flex items-center justify-center gap-1.5 whitespace-nowrap rounded-full px-3 text-[11px] transition-colors ${
              active
                ? "font-semibold text-[var(--ct-bg)]"
                : "font-medium text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
            }`}
            style={{ lineHeight: 1 }}
          >
            {o.icon && (
              <span className="inline-flex shrink-0 transition-colors">

                {o.icon}
              </span>
            )}
            <span>{o.label}</span>
          </button>
        );
      })}
    </div>
  );
}
