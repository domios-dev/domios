import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { activeMentionQuery, applyComposerMention, mentionDismissed, type ComposerMention, type MentionDismissal, type MentionScope, type createMentionRegistry } from "../../lib/composerMentions";

export function useComposerMentions(input: string, setInput: (next: string) => void, onPicked: () => void,
  textarea: RefObject<HTMLTextAreaElement | null>, registry: ReturnType<typeof createMentionRegistry>, scope: MentionScope | (() => MentionScope)) {
  const currentScope = typeof scope === "function" ? scope() : scope;
  const scopeRef = useRef(currentScope);
  scopeRef.current = currentScope;
  const [revision, setRevision] = useState(0);
  const [selection, setSelection] = useState({ start: input.length, end: input.length });
  const [highlighted, setHighlighted] = useState(0);
  const [dismissed, setDismissed] = useState<MentionDismissal | null>(null);
  const [result, setResult] = useState<{ key: string; rows: ComposerMention[] }>({ key: "", rows: [] });
  const pendingCaret = useRef<number | null>(null);
  const query = activeMentionQuery(input, selection.start, selection.end);
  const partial = query ? input.slice(query.start + 1, query.end) : undefined;
  const queryStart = query?.start;
  const hidden = query && mentionDismissed(dismissed, input, selection.start, query);
  const key = query && !hidden ? JSON.stringify([query.start, query.end, partial, scopeRef.current]) : "";
  const active = !!key;
  const mentions = key && result.key === key ? result.rows : [];
  const syncSelection = (element: HTMLTextAreaElement) => setSelection({ start: element.selectionStart, end: element.selectionEnd });
  const close = () => {
    if (query) setDismissed({ start: query.start, draft: input, caret: selection.start });
  };
  useEffect(() => {
    if (!active) return;
    const off = registry.subscribe?.(() => setRevision((value) => value + 1));
    return () => { off?.(); registry.close?.(); };
  }, [active, registry]);
  useEffect(() => {
    if (dismissed && queryStart !== undefined && !hidden) setDismissed(null);
    else if (dismissed && hidden && dismissed.draft !== input) setDismissed({ ...dismissed, draft: input, caret: selection.start });
  }, [dismissed, queryStart, hidden, input, selection.start]);
  useEffect(() => {
    setHighlighted(0);
  }, [key]);
  useEffect(() => {
    if (!key || partial === undefined) return;
    const controller = new AbortController();
    void registry.search(partial, { scope: scopeRef.current, signal: controller.signal }).then((rows) => {
      if (!controller.signal.aborted) setResult({ key, rows });
    }).catch(() => { if (!controller.signal.aborted) setResult({ key, rows: [] }); });
    return () => controller.abort();
  }, [key, partial, registry, revision]);
  useLayoutEffect(() => {
    if (pendingCaret.current === null) return;
    const caret = pendingCaret.current;
    pendingCaret.current = null;
    textarea.current?.setSelectionRange(caret, caret);
    setSelection({ start: caret, end: caret });
  }, [input, textarea]);
  const pick = (mention: ComposerMention) => {
    if (!query) return;
    pendingCaret.current = query.start + mention.insertText.length + 1;
    setInput(applyComposerMention(input, query, mention));
    onPicked();
  };
  const handleKey = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    const code = event.code || event.key;
    if (code === "Backspace" && dismissed) setDismissed(null);
    if (!mentions.length) return false;
    if (code === "ArrowDown" || code === "ArrowUp") {
      event.preventDefault();
      setHighlighted((value) => (value + (code === "ArrowDown" ? 1 : mentions.length - 1)) % mentions.length);
      return true;
    }
    if (code === "Tab" || (code === "Enter" && !event.shiftKey)) {
      event.preventDefault();
      const row = mentions[Math.min(highlighted, mentions.length - 1)];
      if (row) pick(row);
      return true;
    }
    if (code === "Escape") { event.preventDefault(); close(); return true; }
    return false;
  };
  return { mentions, highlighted: Math.min(highlighted, Math.max(0, mentions.length - 1)), setHighlighted, pick, handleKey, syncSelection, close };
}
