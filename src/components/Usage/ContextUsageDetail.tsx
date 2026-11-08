import { ArrowRight } from "lucide-react";
import { classify } from "../../types/usage";
import { formatRelative } from "./format";
import { UsageGauge } from "./UsageGauge";
import { formatCostMicros, type ContextUsage } from "./contextUsage";

const compact = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });

/**
 * The usage popover for a harness that reports its context fill but no quota: the same popover shell and gauge as the
 * quota view, with the session tokens and cost when the harness reports them. Sections without data are left out.
 */
export function ContextUsageDetail({ title, usage, capturedAt, now, reason, onOpenHistory }: {
  title: string; usage: ContextUsage; capturedAt: number | null; now: number; reason?: string; onOpenHistory?: () => void;
}) {
  return (
    <div className="ct-usage-popover" role="dialog" aria-label="usage details">
      <div className="head"><span className="title">{title}</span></div>
      <UsageGauge label="Context window" variant="session" pct={usage.percent} resetsAt={null} state={classify(usage.percent)}
        now={now} note={`${compact.format(usage.used)} of ${compact.format(usage.window)} tokens`} />
      {usage.sessionTokens != null && <div className="ct-usage-account-credits"><span className="k">Session tokens</span><span className="v">{usage.sessionTokens.toLocaleString("en-US")}</span></div>}
      {usage.costMicros != null && <div className="ct-usage-account-credits"><span className="k">Session cost</span><span className="v">{formatCostMicros(usage.costMicros)}</span></div>}
      {reason && <p className="ct-usage-detail">{reason}</p>}
      <div className="ct-usage-foot">
        <span className="live">
          <span className="pulse ct-live-pulse" />
          <span>{capturedAt ? `last updated ${formatRelative(now, capturedAt)}` : "reported by the agent"}</span>
        </span>
        {onOpenHistory && <button type="button" className="link" onClick={onOpenHistory}><span>Full view</span><ArrowRight size={11} /></button>}
      </div>
    </div>
  );
}
