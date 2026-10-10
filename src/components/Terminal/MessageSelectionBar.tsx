import { useState } from "react";
import { Check, Copy, MessageSquarePlus, X } from "lucide-react";

/** The floating bar of a message selection: count, Copy, Ask about these (as a quote), Clear. */
export function MessageSelectionBar({ count, partial = false, onCopy, onQuote, onClear }: {
  count: number;
  partial?: boolean;
  onCopy: () => void;
  onQuote: () => void;
  onAsk?: () => void;
  onClear: () => void;
}) {
  const [copiedCount, setCopiedCount] = useState<number | null>(null);
  if (count === 0) return null;
  const copied = copiedCount === count;
  return (
    <div className="ct-msg-selection-bar" role="toolbar" aria-label="Selected messages">
      <span className="ct-msg-selection-bar__count" aria-live="polite">
        {count} {partial ? (count === 1 ? "part" : "parts") : (count === 1 ? "message" : "messages")}
      </span>
      <span aria-hidden className="ct-msg-selection-bar__rule" />
      <button type="button" onClick={() => { onCopy(); setCopiedCount(count); }}>
        {copied ? <Check size={13} aria-hidden /> : <Copy size={13} aria-hidden />}
        {copied ? "Copied" : "Copy"}
      </button>
      <button type="button" onClick={onQuote}><MessageSquarePlus size={13} aria-hidden />Ask about these</button>
      <button type="button" className="ct-msg-selection-bar__clear" aria-label="Clear selection" title="Clear selection (Esc)" onClick={onClear}>
        <X size={13} aria-hidden />
      </button>
    </div>
  );
}
