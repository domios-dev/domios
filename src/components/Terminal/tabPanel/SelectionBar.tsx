import { useEffect, useState, type RefObject } from "react";
import { Copy, MessageSquarePlus } from "lucide-react";

interface ReaderSelection { text: string; left: number; top: number }

function readSelection(container: HTMLElement | null): ReaderSelection | null {
  const selection = document.getSelection();
  if (!container || !selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!container.contains(range.commonAncestorContainer)) return null;
  const text = selection.toString();
  if (!text.trim()) return null;
  const rect = range.getBoundingClientRect();
  const box = container.getBoundingClientRect();
  return { text, left: Math.max(48, Math.min(box.width - 48, rect.left - box.left + rect.width / 2)), top: Math.max(0, rect.top - box.top) };
}

/** The reader's current non-empty text selection, positioned relative to the reader box. */
export function useReaderSelection(containerRef: RefObject<HTMLElement | null>) {
  const [selection, setSelection] = useState<ReaderSelection | null>(null);
  useEffect(() => {
    const update = () => setSelection(readSelection(containerRef.current));
    document.addEventListener("selectionchange", update);
    return () => document.removeEventListener("selectionchange", update);
  }, [containerRef]);
  return selection;
}

export function SelectionBar({ selection, onCopy, onAddToChat }: { selection: ReaderSelection | null; onCopy: (text: string) => void; onAddToChat: (text: string) => void }) {
  if (!selection) return null;
  return <div role="toolbar" aria-label="Selected text" className="tab-panel__selection-bar" style={{ left: selection.left, top: selection.top }} onMouseDown={(event) => event.preventDefault()}>
    <button type="button" onClick={() => onCopy(selection.text)}><Copy size={12} aria-hidden />Copy</button>
    <button type="button" onClick={() => onAddToChat(selection.text)}><MessageSquarePlus size={12} aria-hidden />Add to chat</button>
  </div>;
}
