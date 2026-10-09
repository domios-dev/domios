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
  return (
    <button
      type="button"
      data-testid={testId}
      data-compact={compact || undefined}
      onClick={onClick}
      title={title}
      aria-label={title}
      className={`ct-scroll-latest pointer-events-auto ${className}`}
    >
      <ChevronDown size={compact ? 13 : 16} strokeWidth={2.2} />
    </button>
  );
}
