import { ChevronRight } from "lucide-react";
import { useId, useState } from "react";

/** First non-empty line, with a whole-line bold heading unwrapped. */
export function thoughtHeadline(text: string): string {
  const line = text.split("\n").map((part) => part.trim()).find(Boolean) ?? "";
  return line.replace(/^\*\*(.+)\*\*$/, "$1").trim();
}

export function ThoughtLine({ text, expanded, onToggle, label = "Thinking", testId = "chat-thought", defaultExpanded = true }: {
  text: string;
  expanded?: boolean;
  onToggle?: (next: boolean) => void;
  label?: string;
  testId?: string;
  defaultExpanded?: boolean;
}) {
  const [localExpanded, setLocalExpanded] = useState(defaultExpanded);
  const regionId = useId();
  const open = expanded ?? localExpanded;
  const toggle = onToggle ?? setLocalExpanded;
  const headline = thoughtHeadline(text);
  if (!headline) return null;

  return (
    <div data-testid={testId} className="min-w-0">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={regionId}
        onClick={() => toggle(!open)}
        className="flex w-full min-w-0 items-center gap-1.5 rounded px-2 py-0.5 text-left text-[11px] opacity-70 hover:opacity-100 focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)] motion-safe:transition-opacity motion-reduce:transition-none"
        style={{ color: "var(--ct-muted-fg)" }}
      >
        <span className="shrink-0">{label}</span>
        <span data-testid="chat-thought-line" className="min-w-0 flex-1 truncate italic">{open ? "" : headline}</span>
        <ChevronRight
          aria-hidden="true"
          size={12}
          className={`shrink-0 motion-safe:transition-transform motion-reduce:transition-none ${open ? "rotate-90" : ""}`}
        />
      </button>
      <div id={regionId} role="region" aria-label={label} hidden={!open} className="px-2 pb-1">
        {open && (
          <p
            data-testid="chat-thought-text"
            className="m-0 whitespace-pre-wrap break-words text-xs"
            style={{ color: "var(--ct-muted-fg)" }}
          >
            {text}
          </p>
        )}
      </div>
    </div>
  );
}
