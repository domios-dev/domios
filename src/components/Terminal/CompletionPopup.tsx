import { useState, useEffect, useCallback } from "react";
import type { CompletionItem } from "../../lib/completions/types";

interface CompletionPopupProps {
  items: CompletionItem[];
  anchorX: number;
  anchorY: number;
  onAccept: (item: CompletionItem) => void;
  onDismiss: () => void;
}

export function CompletionPopup({
  items,
  anchorX,
  anchorY,
  onAccept,
  onDismiss,
}: CompletionPopupProps) {
  const [selectedIndex, setSelectedIndex] = useState(0);

  // Reset selection when items change
  useEffect(() => {
    setSelectedIndex(0);
  }, [items]);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      switch (e.key) {
        case "ArrowDown":
        case "Tab":
          e.preventDefault();
          e.stopPropagation();
          setSelectedIndex((i) => (i + 1) % items.length);
          break;
        case "ArrowUp":
          e.preventDefault();
          e.stopPropagation();
          setSelectedIndex((i) => (i - 1 + items.length) % items.length);
          break;
        case "Enter":
          e.preventDefault();
          e.stopPropagation();
          onAccept(items[selectedIndex]);
          break;
        case "Escape":
          e.preventDefault();
          e.stopPropagation();
          onDismiss();
          break;
        default:
          // Any other key — dismiss and let it pass through
          onDismiss();
          break;
      }
    },
    [items, selectedIndex, onAccept, onDismiss],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [handleKeyDown]);

  if (items.length === 0) return null;

  const maxVisible = 8;
  const visibleItems = items.slice(0, maxVisible);

  return (
    <div
      className="fixed z-50 bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-md shadow-xl overflow-hidden"
      style={{
        left: anchorX,
        top: anchorY,
        minWidth: 220,
        maxWidth: 400,
      }}
    >
      {visibleItems.map((item, i) => (
        <div
          key={item.text}
          onClick={() => onAccept(item)}
          onMouseEnter={() => setSelectedIndex(i)}
          className={`flex items-center gap-3 px-2.5 py-1 cursor-pointer text-xs ${
            i === selectedIndex
              ? "bg-[var(--ct-accent)] text-white"
              : "text-[var(--ct-fg)] hover:bg-[var(--ct-border)]"
          }`}
        >
          <span
            className={`shrink-0 w-1.5 h-1.5 rounded-full ${
              item.kind === "subcommand" ? "bg-[var(--ct-accent)]" : "bg-[var(--ct-muted-fg)]"
            }`}
          />
          <span className="font-mono shrink-0">{item.display}</span>
          <span
            className={`truncate text-[10px] ${
              i === selectedIndex ? "text-white/70" : "text-[var(--ct-muted)]"
            }`}
          >
            {item.description}
          </span>
        </div>
      ))}
      {items.length > maxVisible && (
        <div className="px-2.5 py-0.5 text-[10px] text-[var(--ct-muted)] text-center border-t border-[var(--ct-border)]">
          +{items.length - maxVisible} more
        </div>
      )}
    </div>
  );
}
