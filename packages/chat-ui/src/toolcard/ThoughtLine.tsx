import { ExpandableReader } from "../ExpandableReader";
import { ChevronRight } from "lucide-react";
import { useId, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { thoughtPresentation } from "./thoughtModel";
import { RichMarkdown } from "../MarkdownBody";

export function ThoughtLine({ text, expanded, onToggle, label = "Thinking", testId = "chat-thought", defaultExpanded = false }: {
  text: string;
  expanded?: boolean;
  onToggle?: (next: boolean) => void;
  label?: string;
  testId?: string;
  defaultExpanded?: boolean;
}) {
  const [localExpanded, setLocalExpanded] = useState(defaultExpanded);
  const [clipped, setClipped] = useState(false);
  const body = useRef<HTMLDivElement>(null);
  const regionId = useId();
  const open = expanded ?? localExpanded;
  const toggle = onToggle ?? setLocalExpanded;
  const { headline, expandable } = thoughtPresentation(text);
  useLayoutEffect(() => {
    const element = body.current;
    if (!element || open) return;
    const measure = () => setClipped(element.scrollHeight > element.clientHeight + 1);
    measure();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(element);
    return () => observer?.disconnect();
  }, [text, open]);
  if (!headline) return null;
  const canExpand = expandable || clipped || open;
  const caption = <span className="text-[10.5px] leading-[16px]" style={{ color: "var(--ct-muted-fg)", opacity: 0.7 }}>{label}</span>;
  return <ExpandableReader label={label} enabled={canExpand} openerPlacement="header">{(fullscreen, opener) => <div data-testid={testId} className="min-w-0 py-1">
    <div className="ct-reader-row ct-thought-row flex items-center justify-between">
    {canExpand ? <button type="button" aria-expanded={open} aria-controls={regionId}
      onClick={() => toggle(!open)} className="flex w-fit items-center gap-1 rounded text-left hover:brightness-125">
      {caption}<ChevronRight aria-hidden size={11} className={open ? "rotate-90" : ""} style={{ color: "var(--ct-muted-fg)" }} />
    </button> : caption}
    {opener}
    </div>
    <div ref={body} id={regionId} role="region" aria-label={label} data-testid="chat-thought-line"
      className="m-0 break-words text-xs leading-[18px]"
      style={{ color: "var(--ct-muted-fg)", "--ct-fg": "var(--ct-muted-fg)", ...(!open && !fullscreen ? { display: "-webkit-box", WebkitBoxOrient: "vertical", WebkitLineClamp: 3, overflow: "hidden" } as const : { maxHeight: fullscreen ? "none" : 400, overflowY: "auto", overscrollBehavior: "contain", scrollbarGutter: "stable" }) } as CSSProperties & { "--ct-fg": string }}>
      <div data-testid="chat-thought-text"><RichMarkdown text={text} localArtifacts={false} interactivePreviews={false} /></div>
    </div>
  </div>}</ExpandableReader>;
}
