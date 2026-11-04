import { ExpandableReader } from "../ExpandableReader";
import { ChevronRight } from "lucide-react";
import { useId, useState } from "react";
import { thoughtPresentation } from "./thoughtModel";

export function ThoughtLine({ text, expanded, onToggle, label = "Thinking", testId = "chat-thought", defaultExpanded = false, previewWhenCollapsed = false }: {
  text: string;
  expanded?: boolean;
  onToggle?: (next: boolean) => void;
  label?: string;
  testId?: string;
  defaultExpanded?: boolean;
  previewWhenCollapsed?: boolean;
}) {
  const [localExpanded, setLocalExpanded] = useState(defaultExpanded);
  const regionId = useId();
  const open = expanded ?? localExpanded;
  const toggle = onToggle ?? setLocalExpanded;
  const { headline } = thoughtPresentation(text);
  if (!headline) return null;
  const caption = <span className="text-[10.5px] leading-[16px]" style={{ color: "var(--ct-muted-fg)", opacity: 0.7 }}>{label}</span>;
  return <ExpandableReader label={label} enabled openerPlacement="header">{(fullscreen, opener) => <div data-testid={testId} className="min-w-0 py-1">
    <div className="ct-reader-row flex items-center justify-between">
    <button type="button" aria-expanded={open} aria-controls={regionId}
      onClick={() => toggle(!open)} className="flex w-fit items-center gap-1 rounded text-left hover:brightness-125">
      {caption}<ChevronRight aria-hidden size={11} className={open ? "rotate-90" : ""} style={{ color: "var(--ct-muted-fg)" }} />
    </button>
    {opener}
    </div>
    <p hidden={!previewWhenCollapsed && !open && !fullscreen} id={regionId} role="region" aria-label={label} data-testid="chat-thought-line"
      className="m-0 whitespace-pre-wrap break-words text-xs leading-[18px]"
      style={{ color: "var(--ct-muted-fg)", ...(previewWhenCollapsed && !open && !fullscreen ? { display: "-webkit-box", WebkitBoxOrient: "vertical", WebkitLineClamp: 3, overflow: "hidden" } as const : { maxHeight: fullscreen ? "none" : 400, overflowY: "auto", overscrollBehavior: "contain", scrollbarGutter: "stable" }) }}>
      <span data-testid="chat-thought-text">{text}</span>
    </p>
  </div>}</ExpandableReader>;
}
