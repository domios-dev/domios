import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import {
  activeMentionQuery,
  applyComposerMention,
  filterComposerMentions,
  type ComposerMention,
} from "../../lib/composerMentions";

export function ComposerMentionPicker({
  mentions,
  highlighted,
  onHighlight,
  onPick,
}: {
  mentions: ComposerMention[];
  highlighted: number;
  onHighlight: (index: number) => void;
  onPick: (mention: ComposerMention) => void;
}) {
  if (mentions.length === 0) return null;
  return (
    <div
      role="listbox"
      aria-label="Mention suggestions"
      className="absolute bottom-full left-0 right-0 mb-1 max-h-48 overflow-y-auto rounded"
      style={{
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
        border: "1px solid color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
        zIndex: 10,
      }}
    >
      {mentions.map((mention, i) => {
        const selected = i === highlighted;
        return (
          <button
            key={mention.token}
            type="button"
            role="option"
            aria-selected={selected}
            aria-label={mention.token}
            title={mention.description}
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => onHighlight(i)}
            onClick={() => onPick(mention)}
            className="flex w-full items-center justify-between gap-2 px-2 py-1.5 text-left text-xs"
            style={{
              background: selected ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 20%, transparent)" : "transparent",
              color: "var(--ct-fg, #c9d1d9)",
              border: "none",
              cursor: "pointer",
            }}
          >
            <span className="font-medium" style={{ color: "var(--ct-accent, #58a6ff)" }}>{mention.token}</span>
            <span className="min-w-0 truncate text-[var(--ct-muted-fg)]">{mention.description}</span>
          </button>
        );
      })}
    </div>
  );
}

/** Mention suggestions for the draft's trailing `@word`; Escape hides them until the draft changes. */
export function useComposerMentions(input: string, setInput: (next: string) => void, onPicked: () => void) {
  const [highlighted, setHighlighted] = useState(0);
  const [dismissedFor, setDismissedFor] = useState<string | null>(null);
  const query = useMemo(() => activeMentionQuery(input), [input]);
  const mentions = useMemo(
    () => (dismissedFor === input ? [] : filterComposerMentions(query)),
    [dismissedFor, input, query],
  );
  useEffect(() => setHighlighted(0), [query?.partial]);

  const pick = (mention: ComposerMention) => {
    if (!query) return;
    setInput(applyComposerMention(input, query, mention));
    onPicked();
  };

  /** True when the key belonged to the open picker. */
  const handleKey = (e: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (mentions.length === 0) return false;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setHighlighted((h) => Math.min(Math.max(h + step, 0), mentions.length - 1));
      return true;
    }
    if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
      e.preventDefault();
      const mention = mentions[Math.min(highlighted, mentions.length - 1)];
      if (mention) pick(mention);
      return true;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      setDismissedFor(input);
      return true;
    }
    return false;
  };

  return { mentions, highlighted, setHighlighted, pick, handleKey };
}
