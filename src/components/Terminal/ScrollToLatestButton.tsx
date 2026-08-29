import { ChevronDown } from "lucide-react";

interface Props {
  compact?: boolean;
  className?: string;
  testId: string;
  title?: string;
  onClick: () => void;
}

export function ScrollToLatestButton({
  compact = false,
  className = "",
  testId,
  title = "Scroll to latest",
  onClick,
}: Props) {
  const box = compact ? "h-7 w-7" : "h-8 w-8";
  const icon = compact ? 13 : 16;

  return (
    <button
      type="button"
      data-testid={testId}
      onClick={onClick}
      title={title}
      aria-label={title}
      className={`pointer-events-auto flex ${box} items-center justify-center rounded-full text-[var(--ct-muted-fg)] shadow-sm transition-[color,filter] hover:text-[var(--ct-fg)] hover:brightness-110 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)] ${className}`}
      style={{
        background: "color-mix(in srgb, var(--ct-surface) 80%, var(--ct-fg) 20%)",
        border: "1px solid color-mix(in srgb, var(--ct-border) 80%, var(--ct-fg) 20%)",
      }}
    >
      <ChevronDown size={icon} strokeWidth={2.2} />
    </button>
  );
}
