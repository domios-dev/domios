import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { activeMentionQuery, applyComposerMention, mentionDismissed, type ComposerMention, type MentionDismissal, type MentionScope, type createMentionRegistry } from "../../lib/composerMentions";

export function useComposerMentions(input: string, setInput: (next: string) => void, onPicked: () => void,
  textarea: RefObject<HTMLTextAreaElement | null>, registry: ReturnType<typeof createMentionRegistry>, scope: MentionScope | (() => MentionScope)) {
  const currentScope = typeof scope === "function" ? scope() : scope;
  const scopeRef = useRef(currentScope);
  scopeRef.current = currentScope;
  const listboxId = useId();
  const [revision, setRevision] = useState(0);
  const [selection, setSelection] = useState({ start: input.length, end: input.length });
  const [selected, setSelected] = useState<{ session: string; id: string } | null>(null);
  const [dismissed, setDismissed] = useState<MentionDismissal | null>(null);
  const [result, setResult] = useState<{ session: string; key: string; registry: typeof registry; rows: ComposerMention[]; loading?: boolean; error?: boolean } | null>(null);
  const pendingCaret = useRef<number | null>(null);
  const query = activeMentionQuery(input, selection.start, selection.end);
  const partial = query ? input.slice(query.start + 1, query.end) : undefined;
  const queryStart = query?.start;
  const hidden = query && mentionDismissed(dismissed, input, selection.start, query);
  const session = query && !hidden ? JSON.stringify([input.slice(0, query.start), query.start, currentScope]) : "";
  const key = session ? JSON.stringify([session, partial]) : "";
  const active = !!key;
  const compatible = result?.session === session && result.registry === registry;
  const mentions = active && compatible ? result.rows : [];
  const highlighted = Math.max(0, mentions.findIndex((row) => selected?.session === session && row.id === selected.id));
  const loading = active && (!compatible || result.key !== key || result.loading === true);
  const error = compatible && result.key === key && result.error === true;
  const optionId = (mention: ComposerMention) => `${listboxId}-${encodeURIComponent(mention.id)}`;
  const latest = useRef({ key, session, registry });
  latest.current = { key, session, registry };
  const syncSelection = (element: HTMLTextAreaElement) => setSelection({ start: element.selectionStart, end: element.selectionEnd });
  const setHighlighted = (index: number) => {
    const row = mentions[index];
    if (row) setSelected({ session, id: row.id });
  };
  const close = () => {
    if (query) setDismissed({ start: query.start, draft: input, caret: selection.start });
  };
  useEffect(() => {
    const row = mentions[highlighted];
    if (row && (selected?.session !== session || selected.id !== row.id)) setSelected({ session, id: row.id });
  }, [mentions, highlighted, selected, session]);
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
    if (!key || partial === undefined) { setResult(null); setSelected(null); return; }
    const controller = new AbortController();
    const isCurrent = () => !controller.signal.aborted && latest.current.key === key && latest.current.registry === registry;
    setResult((previous) => ({ session, key, registry, rows: previous?.session === session && previous.registry === registry ? previous.rows : [], loading: true }));
    void registry.search(partial, { scope: scopeRef.current, signal: controller.signal }).then((rows) => {
      if (isCurrent()) setResult({ session, key, registry, rows });
    }).catch(() => { if (isCurrent()) setResult({ session, key, registry, rows: [], error: true }); });
    return () => controller.abort();
  }, [key, partial, session, registry, revision]);
  useLayoutEffect(() => {
    const editor = textarea.current;
    if (!editor) return;
    const attributes = ["role", "aria-autocomplete", "aria-expanded", "aria-busy", "aria-controls", "aria-activedescendant"];
    const previous = attributes.map((name) => [name, editor.getAttribute(name)] as const);
    return () => {
      for (const [name, value] of previous) {
        if (value === null) editor.removeAttribute(name); else editor.setAttribute(name, value);
      }
    };
  }, [textarea]);
  useLayoutEffect(() => {
    const editor = textarea.current;
    if (!editor) return;
    editor.setAttribute("role", "combobox");
    editor.setAttribute("aria-autocomplete", "list");
    editor.setAttribute("aria-expanded", String(active));
    editor.setAttribute("aria-busy", String(loading));
    if (active) editor.setAttribute("aria-controls", listboxId); else editor.removeAttribute("aria-controls");
    const row = mentions[highlighted];
    if (row) editor.setAttribute("aria-activedescendant", optionId(row)); else editor.removeAttribute("aria-activedescendant");
  });
  useLayoutEffect(() => {
    if (pendingCaret.current === null) return;
    const caret = pendingCaret.current;
    pendingCaret.current = null;
    textarea.current?.setSelectionRange(caret, caret);
    setSelection({ start: caret, end: caret });
  }, [input, textarea]);
  const pick = (mention: ComposerMention) => {
    const liveScope = typeof scope === "function" ? scope() : scope;
    if (!query || !active || latest.current.session !== session || latest.current.registry !== registry
      || JSON.stringify(liveScope) !== JSON.stringify(currentScope) || !mentions.includes(mention)) return;
    const editor = textarea.current;
    if (editor && (editor.value !== input || editor.selectionStart !== selection.start || editor.selectionEnd !== selection.end)) return;
    pendingCaret.current = query.start + mention.insertText.length + 1;
    setInput(applyComposerMention(input, query, mention));
    onPicked();
  };
  const handleKey = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    const code = event.code || event.key;
    if (code === "Backspace" && dismissed) setDismissed(null);
    if (active && code === "Escape") { event.preventDefault(); close(); return true; }
    if (!mentions.length) return false;
    if (["ArrowDown", "ArrowUp", "Home", "End", "PageUp", "PageDown"].includes(code)) {
      event.preventDefault();
      const index = code === "Home" ? 0 : code === "End" ? mentions.length - 1
        : code === "PageUp" ? Math.max(0, highlighted - 6) : code === "PageDown" ? Math.min(mentions.length - 1, highlighted + 6)
        : (highlighted + (code === "ArrowDown" ? 1 : mentions.length - 1)) % mentions.length;
      setHighlighted(index);
      return true;
    }
    if (code === "Tab" || (code === "Enter" && !event.shiftKey)) {
      event.preventDefault();
      const row = mentions[highlighted];
      if (row) pick(row);
      return true;
    }
    return false;
  };
  return { active, session, listboxId, optionId, queryStart, query: partial ?? "", mentions, loading, error, highlighted, setHighlighted, pick, handleKey, syncSelection, close };
}
