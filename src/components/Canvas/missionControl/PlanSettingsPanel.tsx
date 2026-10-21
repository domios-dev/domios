import { useState, type ReactNode } from "react";
import { BookOpen, ChevronDown } from "lucide-react";
import type { ApiConfig } from "../../../lib/types";
import { Chip } from "../../shared/Chip";
import { DropdownSelect } from "../../shared/DropdownMenu";
import { LiquidToggle } from "../../shared/LiquidToggle";
import { Segmented } from "../../shared/Segmented";
import { PLAN_PAUSE_HINT, PlanModeControls } from "../planView/PlanModeControls";
import type { Plan } from "../types";
import { BoardPanel } from "./BoardPanel";
import {
  HEARTBEAT_INTERVALS,
  oversightLines,
  oversightSummary,
  pauseHint,
  type OversightMode,
  type OversightTone,
  type PlanOversight,
} from "./planSettingsModel";

const COLLAPSED_KEY = "codeterm.mc.plan-settings.collapsed";

function readCollapsed(): boolean {
  try { return localStorage.getItem(COLLAPSED_KEY) === "1"; } catch { return false; }
}

function writeCollapsed(collapsed: boolean) {
  try { localStorage.setItem(COLLAPSED_KEY, collapsed ? "1" : "0"); } catch { /* per-viewer convenience only */ }
}

const MODE_HINT: Record<OversightMode, string> = {
  curator: "An agent reviews the plan each round and nudges the Orchestrator when work drifts.",
  heartbeat: "A plain reminder reaches the Orchestrator on a fixed interval.",
};

/** One setting: its name and control on one line, the hint and any live lines across the full width beneath. */
export function PlanSettingRow({ label, badge, hint, control, children, testId }: {
  label: string;
  badge?: ReactNode;
  hint?: ReactNode;
  control: ReactNode;
  children?: ReactNode;
  testId?: string;
}) {
  return (
    <div className="mc-setting" data-testid={testId}>
      <div className="mc-setting-label">{label}{badge}</div>
      <div className="mc-setting-control">{control}</div>
      <div className="mc-setting-body">
        {hint && <p className="mc-setting-hint">{hint}</p>}
        {children}
      </div>
    </div>
  );
}

const suggestionBadge = <Chip variant="muted">Suggestion</Chip>;
const WORKER_LIMITS = [1, 2, 3, 4, 6];

/** Proposed setting, passed in by the caller until the owner adopts it. */
export function ParallelWorkersRow({ value, onChange }: { value: number; onChange: (value: number) => void }) {
  return <PlanSettingRow testId="mc-plan-settings-workers" label="Parallel Workers" badge={suggestionBadge}
    hint="How many Workers the Orchestrator may run at once."
    control={<DropdownSelect aria-label="Parallel Workers" className="w-auto" prefix="Up to" value={String(value)}
      onChange={next => onChange(Number(next))} options={WORKER_LIMITS.map(limit => ({ value: String(limit), label: String(limit) }))} />} />;
}

/** Proposed setting, passed in by the caller until the owner adopts it. */
export function QuietHoursRow({ enabled, window, onChange }: { enabled: boolean; window: string; onChange: (enabled: boolean) => void }) {
  return <PlanSettingRow testId="mc-plan-settings-quiet" label="Quiet hours" badge={suggestionBadge}
    hint={`No rounds or dispatch ${window}.`}
    control={<LiquidToggle aria-label="Quiet hours" checked={enabled} onCheckedChange={onChange} />} />;
}

function StatusDot({ tone }: { tone: OversightTone }) {
  return <span className="mc-status-dot" data-tone={tone} aria-hidden="true" />;
}

export function PlanSettingsPanel({ plan, api, oversight, now, onModeChange, onIntervalChange, onOpenGuide, children }: {
  plan: Pick<Plan, "id" | "mode" | "status">;
  api?: ApiConfig;
  oversight: PlanOversight;
  now: number;
  onModeChange: (mode: OversightMode) => void;
  onIntervalChange: (intervalMs: number) => void;
  onOpenGuide?: () => void;
  /** Further rows built from PlanSettingRow. */
  children?: ReactNode;
}) {
  const [collapsed, setCollapsed] = useState(readCollapsed);
  const toggle = () => { setCollapsed(!collapsed); writeCollapsed(!collapsed); };
  const heartbeatPicker = oversight.mode === "heartbeat" && !oversight.paused;
  return (
    <BoardPanel
      testId="mc-plan-settings"
      title="Plan settings"
      className={collapsed ? "mc-plan-settings mc-plan-settings--folded" : "mc-plan-settings"}
      meta={<span className="mc-plan-settings-meta">
        {collapsed && <span data-testid="mc-plan-settings-summary" className="mc-plan-settings-summary">{oversightSummary(oversight, now)}</span>}
        <Chip variant={oversight.paused ? "warning" : "success"}><StatusDot tone={oversight.paused ? "warning" : "green"} />{oversight.paused ? "Paused" : "Running"}</Chip>
        <button type="button" className="mc-plan-settings-fold" aria-expanded={!collapsed}
          aria-label={collapsed ? "Expand plan settings" : "Collapse plan settings"} onClick={toggle}>
          <ChevronDown size={14} aria-hidden="true" />
        </button>
      </span>}
    >
      {!collapsed && <div className="mc-plan-settings-grid">
        <PlanSettingRow
          testId="mc-plan-settings-oversight"
          label="Keep on track"
          hint={MODE_HINT[oversight.mode]}
          control={<Segmented<OversightMode> aria-label="How the Orchestrator is kept on track" value={oversight.mode} onChange={onModeChange} height={26}
            options={[{ value: "curator", label: "Curator" }, { value: "heartbeat", label: "Heartbeat" }]} />}
        >
          {onOpenGuide && <button type="button" className="mc-board-more mc-setting-link" onClick={onOpenGuide}>
            <BookOpen size={12} aria-hidden="true" />How the Curator works
          </button>}
          <div className="mc-setting-live-group">
            {heartbeatPicker && <DropdownSelect aria-label="Heartbeat interval" className="w-auto" prefix="Every"
              value={String(oversight.heartbeat_interval_ms)} onChange={value => onIntervalChange(Number(value))}
              options={HEARTBEAT_INTERVALS.map(option => ({ value: String(option.ms), label: option.label }))} />}
            {oversightLines(oversight, now).map(line => (
              <p key={line.text} className="mc-setting-live" data-tone={line.tone}>
                <StatusDot tone={line.tone} />{line.text}
              </p>
            ))}
          </div>
        </PlanSettingRow>
        <PlanSettingRow
          testId="mc-plan-settings-run"
          label="Run state"
          hint={pauseHint(oversight, now, PLAN_PAUSE_HINT)}
          control={<PlanModeControls plan={plan} api={api} reason="Paused from Mission Control" />}
        />
        {children}
      </div>}
    </BoardPanel>
  );
}
