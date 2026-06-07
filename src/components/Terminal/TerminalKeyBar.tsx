interface TerminalKeyBarProps {
  onSendKey: (data: string) => void;
  className?: string;
}

const CAPSULE_STYLE = {
  background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, white 5%)",
  border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 28%, transparent)",
  color: "var(--ct-muted-fg)",
} as const;

const ARROWS: { label: string; data: string }[] = [
  { label: "←", data: "\x1b[D" },
  { label: "↑", data: "\x1b[A" },
  { label: "↓", data: "\x1b[B" },
  { label: "→", data: "\x1b[C" },
];

// Touch key capsules for terminal control on mobile: esc kept far left,
// arrows centered, enter far right so fat fingers don't cross-hit.
export function TerminalKeyBar({ onSendKey, className }: TerminalKeyBarProps) {
  const key = (label: string, data: string, minW = "min-w-11") => (
    <button
      key={label}
      type="button"
      data-testid={`terminal-key-${label}`}
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => onSendKey(data)}
      className={`inline-flex h-10 ${minW} items-center justify-center rounded-full px-3 text-sm font-mono active:brightness-125`}
      style={CAPSULE_STYLE}
    >
      {label}
    </button>
  );

  return (
    <div
      data-terminal-keys="true"
      className={`flex items-center justify-between gap-2 ${className ?? ""}`}
    >
      {key("esc", "\x1b")}
      <div className="flex items-center gap-1.5">
        {ARROWS.map(({ label, data }) => key(label, data))}
      </div>
      {key("enter", "\r", "min-w-14")}
    </div>
  );
}
