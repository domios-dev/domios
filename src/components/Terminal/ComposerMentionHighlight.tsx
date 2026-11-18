import { useLayoutEffect, useRef, type RefObject } from "react";
import type { MentionDraft } from "./composerMentionTokens";

export function ComposerMentionHighlight({ draft, textarea, marginPx }: { draft: MentionDraft; textarea: RefObject<HTMLTextAreaElement | null>; marginPx: number }) {
  const mirror = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const editor = textarea.current, layer = mirror.current;
    if (!editor || !layer) return;
    const sync = () => {
      const style = editor.ownerDocument.defaultView?.getComputedStyle(editor);
      if (!style) return;
      for (const name of ["font-family", "font-size", "font-weight", "font-style", "line-height", "letter-spacing", "padding", "text-indent", "tab-size", "word-spacing"]) layer.style.setProperty(name, style.getPropertyValue(name));
      layer.style.width = `${editor.clientWidth}px`;
      layer.style.height = `${editor.clientHeight}px`;
      layer.scrollTop = editor.scrollTop;
      layer.scrollLeft = editor.scrollLeft;
    };
    sync();
    editor.addEventListener("scroll", sync);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(sync);
    observer?.observe(editor);
    return () => { editor.removeEventListener("scroll", sync); observer?.disconnect(); };
  }, [draft.text, textarea, marginPx]);
  let cursor = 0;
  const content = draft.tokens.flatMap(token => {
    const text = draft.text.slice(cursor, token.start);
    cursor = token.end;
    return [text, <span key={token.start} style={{ background: "color-mix(in srgb, var(--ct-accent) 14%, transparent)", boxShadow: "0 0 0 2px color-mix(in srgb, var(--ct-accent) 14%, transparent)", borderRadius: 6 }}>{draft.text.slice(token.start, token.end)}</span>];
  });
  return <div ref={mirror} aria-hidden="true" data-testid="composer-mention-highlight" className="pointer-events-none absolute left-0 overflow-hidden"
    style={{ top: marginPx, color: "transparent", whiteSpace: "pre-wrap", overflowWrap: "break-word" }}>
    {content}{draft.text.slice(cursor)}{draft.text.endsWith("\n") ? " " : ""}
  </div>;
}
