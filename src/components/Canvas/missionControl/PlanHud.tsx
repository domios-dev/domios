import type { PlanHudMetrics } from "./types";

interface PlanHudProps {
  planTitle: string;
  goal: string;
  hud: PlanHudMetrics;
  compact?: boolean;
}

export function PlanHud({ planTitle, goal, hud, compact }: PlanHudProps) {
  return (
    <header className="mc-hud" data-testid="mission-control-hud">
      <div className="mc-hud-metric" style={{ flex: compact ? undefined : "1 1 160px" }}>
        <strong>{hud.progressPct}%</strong>
        <span>Progress</span>
        <div className="mc-detail-only" style={{ marginTop: 4, fontSize: 11, fontWeight: 600 }}>{planTitle}</div>
        {!compact && (
          <div className="mc-detail-only" style={{ marginTop: 2, fontSize: 10, color: "var(--mc-muted)" }}>{goal}</div>
        )}
      </div>
      <div className="mc-progress-bar mc-detail-only">
        <div className="mc-progress-track">
          <div className="mc-progress-fill" style={{ width: `${hud.progressPct}%` }} />
        </div>
      </div>
      <Metric label="Live lanes" value={hud.liveLanes} />
      <Metric label="QA depth" value={hud.qaDepth} warn={hud.qaDepth > 3} />
      <Metric label="Loops" value={hud.activeLoops} />
      <Metric label="Compete" value={hud.competitiveAttempts} />
      <Metric label="Cleanup debt" value={hud.cleanupDebt} warn={hud.cleanupDebt > 0} />
    </header>
  );
}

function Metric({ label, value, warn }: { label: string; value: number; warn?: boolean }) {
  return (
    <div className="mc-hud-metric">
      <strong style={{ color: warn ? "var(--mc-warn)" : undefined }}>{value}</strong>
      <span>{label}</span>
    </div>
  );
}
