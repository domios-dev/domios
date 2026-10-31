import { createContext, useContext, useId, useLayoutEffect, useRef, useState, type HTMLAttributes } from "react";
import { boundedMessageIsClipped, boundedMessageLayout } from "./boundedMessageModel";

const MessageBoundsContext = createContext(false);

export function useParentMessageBounds(): boolean {
  return useContext(MessageBoundsContext);
}

interface Props extends HTMLAttributes<HTMLElement> {
  collapsedHeight: string;
  as?: "div" | "pre";
  unbounded?: boolean;
}

export function BoundedMessageContent({ collapsedHeight, as: Tag = "div", unbounded = false,
  children, style, hidden, id, ...props }: Props) {
  const parentOwnsBounds = useParentMessageBounds();
  const ownsBounds = !unbounded && !parentOwnsBounds;
  const generatedId = useId();
  const contentId = id ?? generatedId;
  const contentRef = useRef<HTMLElement | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [clipped, setClipped] = useState(false);
  useLayoutEffect(() => {
    const content = contentRef.current;
    if (!content || !ownsBounds || hidden) return;
    const measure = () => setClipped(boundedMessageIsClipped(content.scrollHeight, content.clientHeight));
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    for (const child of content.children) observer.observe(child);
    return () => observer.disconnect();
  }, [children, hidden, ownsBounds]);
  const layout = boundedMessageLayout(collapsedHeight, expanded);
  const content = (
    <Tag {...props} hidden={hidden} id={contentId} ref={(node: HTMLDivElement | HTMLPreElement | null) => { contentRef.current = node; }}
      data-bounded-message={!ownsBounds ? undefined : expanded ? "expanded" : "collapsed"}
      style={{ ...style, ...(!ownsBounds ? {} : layout), ...(ownsBounds && clipped && !expanded ? { maskImage: "linear-gradient(to bottom, black calc(100% - 36px), transparent)", WebkitMaskImage: "linear-gradient(to bottom, black calc(100% - 36px), transparent)" } : {}) }}>
      {children}
    </Tag>
  );
  if (!ownsBounds) return <MessageBoundsContext.Provider value={parentOwnsBounds}>{content}</MessageBoundsContext.Provider>;
  return (
    <MessageBoundsContext.Provider value={true}>
    <div className="min-w-0 max-w-full" hidden={hidden}>
      {content}
      {(clipped || expanded) && (
        <button type="button" aria-expanded={expanded} aria-controls={contentId}
          className="px-2 py-1 text-xs text-[var(--ct-accent)]" onClick={() => {
            if (expanded && contentRef.current) contentRef.current.scrollTop = 0;
            setExpanded((value) => !value);
          }}>
          {expanded ? "Show less" : "Show all"}
        </button>
      )}
    </div>
    </MessageBoundsContext.Provider>
  );
}
