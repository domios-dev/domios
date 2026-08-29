import { Scan } from "lucide-react";
import { ScrollToLatestButton } from "./ScrollToLatestButton";

interface Props {
  /** Touch layout: smaller targets so the cluster does not cover terminal text. */
  compact: boolean;
  showFit: boolean;
  showScrollToBottom: boolean;
  onFit: () => void;
  onScrollToBottom: () => void;
}

export function TerminalViewControls({ compact, showFit, showScrollToBottom, onFit, onScrollToBottom }: Props) {
  const icon = compact ? 13 : 15;
  const box = compact ? "h-7 w-7" : "h-8 w-8";
  return (
    <>
      {showFit && (
        <button
          type="button"
          data-testid="terminal-fit"
          onClick={onFit}
          title="Fit terminal to the window"
          aria-label="Fit terminal to the window"
          className={`pointer-events-auto absolute right-2 z-30 flex ${box} items-center justify-center rounded-full text-[var(--ct-muted-fg)] transition-colors hover:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]`}
          style={{
            top: compact ? "calc(8px + var(--ct-safe-top, 0px))" : 8,
            background: "var(--ct-surface)",
            border: "1px solid var(--ct-border)",
          }}
        >
          <Scan size={icon} strokeWidth={2.2} />
        </button>
      )}
      {showScrollToBottom && (
        <ScrollToLatestButton
          compact={compact}
          testId="terminal-scroll-bottom"
          onClick={onScrollToBottom}
          title="Scroll to latest output"
          className="absolute bottom-2 right-2 z-30"
        />
      )}
    </>
  );
}
