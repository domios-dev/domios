import type { CSSProperties } from "react";

interface TerminalKeyBarProps {
  onSendKey: (data: string) => void;
  className?: string;
}

type KeyVariant = "neutral" | "danger" | "accent";

const VARIANT_STYLES: Record<KeyVariant, CSSProperties> = {
  neutral: {
    background: "color-mix(in srgb, var(--ct-bg, #0d1117) 82%, white 7%)",
    border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 26%, transparent)",
    color: "var(--ct-fg, #c9d1d9)",
    boxShadow: "inset 0 1px 0 color-mix(in srgb, white 6%, transparent), 0 1px 2px rgba(0,0,0,0.4)",
  },
  danger: {
    background: "color-mix(in srgb, var(--ct-red, #f85149) 10%, var(--ct-bg, #0d1117))",
    border: "1px solid color-mix(in srgb, var(--ct-red, #f85149) 35%, transparent)",
    color: "var(--ct-red, #f85149)",
    boxShadow: "0 1px 2px rgba(0,0,0,0.4)",
  },
  accent: {
    background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, var(--ct-bg, #0d1117))",
    border: "1px solid color-mix(in srgb, var(--ct-accent, #58a6ff) 40%, transparent)",
    color: "var(--ct-accent, #58a6ff)",
    boxShadow: "0 1px 2px rgba(0,0,0,0.4)",
  },
};

const ARROWS: { label: string; data: string }[] = [
  { label: "←", data: "\x1b[D" },
  { label: "↑", data: "\x1b[A" },
  { label: "↓", data: "\x1b[B" },
  { label: "→", data: "\x1b[C" },
];

// Touch key bar: esc left, arrows center, enter right; colors mark cancel/confirm.
export function TerminalKeyBar({ onSendKey, className }: TerminalKeyBarProps) {
  const key = (
    label: string,
    data: string,
    variant: KeyVariant,
    extra = "min-w-11 rounded-[10px]",
  ) => (
    <button
      key={label}
      type="button"
      data-testid={`terminal-key-${label}`}
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => onSendKey(data)}
      className={`inline-flex h-11 ${extra} select-none touch-manipulation items-center justify-center px-3 text-sm font-mono transition-[filter,transform] duration-75 active:translate-y-px active:brightness-125`}
      style={VARIANT_STYLES[variant]}
    >
      {label}
    </button>
  );

  return (
    <div
      data-terminal-keys="true"
      className={`flex items-center justify-between gap-2 ${className ?? ""}`}
    >
      {key("esc", "\x1b", "danger", "min-w-13 rounded-full")}
      <div className="flex items-center gap-1.5">
        {ARROWS.map(({ label, data }) => key(label, data, "neutral"))}
      </div>
      {key("enter", "\r", "accent", "min-w-16 rounded-full")}
    </div>
  );
}
