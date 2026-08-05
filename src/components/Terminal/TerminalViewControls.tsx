import { ChevronDown, Scan } from "lucide-react";

interface Props {
  /** Touch layout: smaller targets so the cluster does not cover terminal text. */
  compact: boolean;
  /** Only while the viewport is detached from the tail. */
  showScrollToBottom: boolean;
  onFit: () => void;
  onScrollToBottom: () => void;
}

/**
 * Controls that live inside the terminal field rather than in the tab header, so
 * they stay reachable on a phone where the header pills are the only other anchor.
 * The cluster itself is click-through; only the buttons take pointer events, or it
 * would swallow the taps that focus the terminal.
 */
export function TerminalViewControls({ compact, showScrollToBottom, onFit, onScrollToBottom }: Props) {
  const icon = compact ? 13 : 15;
  const box = compact ? "h-7 w-7" : "h-8 w-8";
  const chrome = "pointer-events-auto flex items-center justify-center rounded-full text-[var(--ct-muted-fg)] transition-colors hover:text-[var(--ct-fg)]";
  const surface = { background: "var(--ct-surface)", border: "1px solid var(--ct-border)" };
  return (
    <div
      className="pointer-events-none absolute z-30 flex flex-col items-end gap-1.5"
      style={{
        right: 8,
        top: compact ? "calc(8px + var(--ct-safe-top, 0px))" : 8,
      }}
    >
      <button
        type="button"
        data-testid="terminal-fit"
        onClick={onFit}
        title="Fit terminal to the window"
        aria-label="Fit terminal to the window"
        className={`${chrome} ${box}`}
        style={surface}
      >
        <Scan size={icon} strokeWidth={2.2} />
      </button>
      {showScrollToBottom && (
        <button
          type="button"
          data-testid="terminal-scroll-bottom"
          onClick={onScrollToBottom}
          title="Scroll to latest output"
          aria-label="Scroll to latest output"
          className={`${chrome} ${box}`}
          style={surface}
        >
          <ChevronDown size={icon} strokeWidth={2.2} />
        </button>
      )}
    </div>
  );
}
