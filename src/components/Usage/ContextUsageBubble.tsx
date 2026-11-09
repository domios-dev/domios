import { useCallback, useRef, useState, type ReactNode } from "react";
import { classify } from "../../types/usage";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { ContextUsageDetail } from "./ContextUsageDetail";
import { formatCostMicros, type ContextUsage } from "./contextUsage";

/** The context fill a harness reported for its tab, as the usage pill's value. */
export function ContextUsageBubble({ title, usage, capturedAt, reason, onOpenHistory, children }: {
  title: string; usage: ContextUsage; capturedAt: number | null; reason?: string; onOpenHistory?: () => void; children?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  const pct = Math.round(usage.percent);
  const state = classify(usage.percent);
  const cost = usage.costMicros != null ? ` · ${formatCostMicros(usage.costMicros)}` : "";
  return (
    <div className="ct-usage" style={{ position: "relative", display: "inline-flex" }} data-testid="usage-context-bubble">
      <button ref={button} type="button"
        className={["ct-bar-pill ct-usage-bubble", state === "warn" ? "is-warn" : "", state === "danger" ? "is-danger" : "", open ? "is-open" : ""].filter(Boolean).join(" ")}
        aria-label={`context ${pct}% of window${cost}`} aria-expanded={open}
        onClick={(event) => { event.stopPropagation(); setOpen((value) => !value); }}>
        <span className={`glyph ${state}`}>
          <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <rect x="1.6" y="2.4" width="8.8" height="7.2" rx="1.6" /><path d="M3.8 6h4.4" />
          </svg>
          <span className="pct">ctx {pct}%</span>
        </span>
      </button>
      <AnchoredPopover open={open} anchor={button} placement="bottom-end" className="ct-usage" onClose={close}>
        <ContextUsageDetail title={title} usage={usage} now={Date.now()} capturedAt={capturedAt} reason={reason}
          onOpenHistory={onOpenHistory ? () => { onOpenHistory(); setOpen(false); } : undefined} />
      </AnchoredPopover>
      {children}
    </div>
  );
}
