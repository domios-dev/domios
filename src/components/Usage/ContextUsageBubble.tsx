import { useCallback, useRef, useState, type ReactNode } from "react";
import { classify } from "../../types/usage";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { ContextUsageDetail } from "./ContextUsageDetail";
import { formatCostMicros, type ContextUsage } from "./contextUsage";

/** The context fill a harness reported for its tab, using the same sleek bar+percent pill as ComposerContextMeter. */
export function ContextUsageBubble({ title, usage, capturedAt, reason, onOpenHistory, children }: {
  title: string; usage: ContextUsage; capturedAt: number | null; reason?: string; onOpenHistory?: () => void; children?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  const pct = Math.round(usage.percent);
  const fillWidth = `${Math.max(0, Math.min(100, usage.percent))}%`;
  const state = classify(usage.percent);
  const cost = usage.costMicros != null ? ` · ${formatCostMicros(usage.costMicros)}` : "";
  return (
    <div className="ct-usage" style={{ position: "relative", display: "inline-flex" }} data-testid="usage-context-bubble">
      <button ref={button} type="button"
        className={["ct-bar-pill ct-usage-bubble ct-usage-bubble--context", state === "warn" ? "is-warn" : "", state === "danger" ? "is-danger" : "", open ? "is-open" : ""].filter(Boolean).join(" ")}
        aria-label={`context ${pct}% of window${cost}`} aria-expanded={open}
        onClick={(event) => { event.stopPropagation(); setOpen((value) => !value); }}>
        <span className="ct-bar-meter-track" aria-hidden="true">
          <span className="ct-bar-meter-fill" style={{ width: fillWidth }} />
        </span>
        <span className="pct">{pct}%</span>
      </button>
      <AnchoredPopover open={open} anchor={button} placement="bottom-end" className="ct-usage" onClose={close}>
        <ContextUsageDetail title={title} usage={usage} now={Date.now()} capturedAt={capturedAt} reason={reason}
          onOpenHistory={onOpenHistory ? () => { onOpenHistory(); setOpen(false); } : undefined} />
      </AnchoredPopover>
      {children}
    </div>
  );
}
