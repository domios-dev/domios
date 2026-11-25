import { useRef, type RefObject } from "react";
import { useComposerMirror } from "./ComposerCapsuleField";
import type { MentionDraft } from "./composerMentionTokens";

export function ComposerMentionHighlight({ draft, textarea, marginPx }: { draft: MentionDraft; textarea: RefObject<HTMLTextAreaElement | null>; marginPx: number }) {
  const mirror = useRef<HTMLDivElement>(null);
  useComposerMirror(textarea, mirror, draft.text);
  let cursor = 0;
  const content = draft.tokens.flatMap(token => {
    const text = draft.text.slice(cursor, token.start);
    cursor = token.end;
    return [text, <span key={token.start} style={{ background: "color-mix(in srgb, var(--ct-accent) 14%, transparent)", boxShadow: "0 0 0 2px color-mix(in srgb, var(--ct-accent) 14%, transparent)", borderRadius: 6 }}>{draft.text.slice(token.start, token.end)}</span>];
  });
  return <div ref={mirror} aria-hidden="true" data-testid="composer-mention-highlight" className="pointer-events-none absolute left-0 overflow-hidden"
    style={{ insetBlock: marginPx, color: "transparent" }}>
    {content}{draft.text.slice(cursor)}{draft.text.endsWith("\n") ? " " : ""}
  </div>;
}
