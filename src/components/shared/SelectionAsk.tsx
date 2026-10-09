import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { ArrowUp, Loader2 } from "lucide-react";
import { sendTrackedChatMessage } from "../Terminal/chatSendStatus";
import { clampFloatingPosition } from "./floatingPosition";
import { BusySpinner } from "./BusyState";

export interface SelectionAskOwner {
  tabId: string;
  name: string;
}

interface Anchor {
  quote: string;
  top: number;
  left: number;
}

const POPOVER_WIDTH = 320;
const MAX_QUOTE = 1200;

export function composeSelectionAsk(subject: string, quote: string, question: string): string {
  const quoted = quote.slice(0, MAX_QUOTE).split("\n").map((line) => `> ${line}`).join("\n");
  return `Question about ${subject}:\n${question}\n\nQuoted:\n${quoted}`;
}

/** Select text inside, ask the owning agent about it, land in its chat. */
export function SelectionAsk({ subject, owner, onJump, children, className }: {
  subject: string;
  owner: SelectionAskOwner | null;
  onJump: (tabId: string) => void;
  children: ReactNode;
  className?: string;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const [anchor, setAnchor] = useState<Anchor | null>(null);
  const [open, setOpen] = useState(false);
  const [question, setQuestion] = useState("");
  const [sending, setSending] = useState(false);
  const [failed, setFailed] = useState(false);

  const readSelection = useCallback(() => {
    if (open) return;
    const selection = window.getSelection();
    const root = rootRef.current;
    if (!selection || selection.isCollapsed || !root || selection.rangeCount === 0) {
      setAnchor(null);
      return;
    }
    const range = selection.getRangeAt(0);
    if (!root.contains(range.commonAncestorContainer)) return;
    const quote = selection.toString().trim();
    if (!quote) return;
    const rects = range.getClientRects();
    const last = rects[rects.length - 1] ?? range.getBoundingClientRect();
    setAnchor({ quote, top: last.bottom + 6, left: last.right });
  }, [open]);

  useEffect(() => {
    if (!owner) return;
    const onUp = () => requestAnimationFrame(readSelection);
    document.addEventListener("mouseup", onUp);
    document.addEventListener("keyup", onUp);
    return () => {
      document.removeEventListener("mouseup", onUp);
      document.removeEventListener("keyup", onUp);
    };
  }, [owner, readSelection]);

  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => {
      if (popRef.current?.contains(event.target as Node)) return;
      setOpen(false);
      setAnchor(null);
    };
    document.addEventListener("mousedown", onDown, true);
    return () => document.removeEventListener("mousedown", onDown, true);
  }, [open]);

  const close = () => {
    setOpen(false);
    setAnchor(null);
    setQuestion("");
    setFailed(false);
  };

  const send = async () => {
    const text = question.trim();
    if (!owner || !anchor || !text || sending) return;
    setSending(true);
    setFailed(false);
    try {
      await sendTrackedChatMessage(owner.tabId, `ask-${Date.now().toString(36)}`, composeSelectionAsk(subject, anchor.quote, text));
      close();
      window.getSelection()?.removeAllRanges();
      onJump(owner.tabId);
    } catch {
      setFailed(true);
    } finally {
      setSending(false);
    }
  };

  const onKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    } else if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void send();
    }
  };

  const pos = anchor
    ? clampFloatingPosition(anchor.left - (open ? POPOVER_WIDTH / 2 : 12), anchor.top, open ? POPOVER_WIDTH : 140, open ? 160 : 32, window.innerWidth, window.innerHeight)
    : null;

  return (
    <div ref={rootRef} className={className} data-selection-ask="">
      {children}
      {owner && anchor && pos && createPortal(
        open ? (
          <div ref={popRef} role="dialog" aria-label={`Ask ${owner.name}`} className="ct-portal-menu ct-menu ct-sel-ask__pop fixed" style={{ top: pos.top, left: pos.left, width: POPOVER_WIDTH }}>
            <blockquote className="ct-sel-ask__quote">{anchor.quote}</blockquote>
            <div className="ct-sel-ask__field">
              {/* biome-ignore lint/a11y/noAutofocus: the popover opens on an explicit click to type a question */}
              <textarea autoFocus rows={2} value={question} placeholder={`Ask ${owner.name} about this…`} aria-label={`Question for ${owner.name}`} onChange={(e) => setQuestion(e.target.value)} onKeyDown={onKey} />
              <button type="button" className="ct-sel-ask__send" aria-label={`Send to ${owner.name}`} disabled={!question.trim() || sending} onClick={() => void send()}>
                {sending ? <BusySpinner icon={Loader2} size={14} /> : <ArrowUp size={14} aria-hidden="true" />}
              </button>
            </div>
            <p className="ct-sel-ask__hint">{failed ? `${owner.name} is not reachable right now.` : `Sends to ${owner.name} and opens the chat`}</p>
          </div>
        ) : (
          <button type="button" className="ct-sel-ask fixed" style={{ top: pos.top, left: pos.left }} onMouseDown={(e) => e.preventDefault()} onClick={() => setOpen(true)} aria-label={`Ask ${owner.name} about the selection`}>
            <span className="ct-sel-ask__dot" aria-hidden="true" />
            <span className="ct-sel-ask__label">Ask {owner.name}</span>
          </button>
        ),
        document.body,
      )}
    </div>
  );
}
