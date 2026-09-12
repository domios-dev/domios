// Pure reading of the stored Mission Control projection. Nothing here opens a
// session log, parses note prose, or invents a figure the contract does not carry.

import { classify, type UsageState } from "../../../types/usage";
import type { MilestoneKind } from "../types";
import type {
  AreaGrouping,
  EventLog,
  KpiNotRecordingReason,
  PlanEvent,
  PlanTaskCounts,
  ProviderRunway,
} from "./missionControlState";

export type BoardLayout = "wide" | "narrow";

/** Below this the board follows the phone artboard: two-up cards, stacked gauges. */
export const BOARD_NARROW_MAX_PX = 720;

export function boardLayout(width: number): BoardLayout {
  return width > 0 && width <= BOARD_NARROW_MAX_PX ? "narrow" : "wide";
}

export const BOARD_CARD_COLUMNS: Record<BoardLayout, string> = {
  wide: "1.6fr minmax(0, 1fr) minmax(0, 1fr) minmax(0, 1fr)",
  narrow: "repeat(2, minmax(0, 1fr))",
};

export const BOARD_RUNWAY_COLUMNS: Record<BoardLayout, string> = {
  wide: "repeat(3, minmax(0, 1fr))",
  narrow: "minmax(0, 1fr)",
};

const UNITS = [
  { limit: 1_000_000_000, suffix: "B" },
  { limit: 1_000_000, suffix: "M" },
  { limit: 1_000, suffix: "K" },
] as const;

export function formatCompact(value: number): string {
  if (!Number.isFinite(value)) return "—";
  const abs = Math.abs(value);
  for (const unit of UNITS) {
    if (abs >= unit.limit) {
      const scaled = value / unit.limit;
      const digits = Math.abs(scaled) >= 100 ? 0 : 1;
      return `${scaled.toFixed(digits).replace(/\.0$/, "")}${unit.suffix}`;
    }
  }
  return String(Math.round(value));
}

export function formatCount(value: number): string {
  return Math.round(value).toLocaleString("en-US");
}

export function formatAgeMs(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  if (h > 0) return `${h} h ${String(m).padStart(2, "0")} m`;
  if (m > 0) return `${m} m`;
  return `${total} s`;
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

export function formatClock(ts: number): string {
  const d = new Date(ts);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function formatHourBand(ts: number): string {
  return `${pad(new Date(ts).getHours())}:00`;
}

const KIND_LABELS: Record<string, string> = {
  planning: "Planning",
  task_created: "Task added",
  decision: "Decision",
  review: "Verdict",
  worker_report: "Report",
  gate: "Question",
  status_update: "Status",
  qa: "QA",
  bounce: "Bounce",
  accepted: "Accepted",
  acceptance: "Accepted",
  dispatch: "Dispatch",
  incident: "Incident",
  evidence: "Evidence",
  testing: "Testing",
  verification: "Verification",
  integration: "Integration",
  cleanup: "Cleanup",
  skill: "Skill kept",
  regression: "Regression gate",
  retrospective: "Retrospective",
  note: "Note",
};

/** Kinds the log renders at full strength; everything else stays quiet. */
const STRONG_KINDS = new Set(["planning", "review", "decision", "gate", "incident"]);

export function eventKindLabel(kind: string, declared: readonly MilestoneKind[] = []): string {
  const declaredLabel = declared.find((entry) => entry.key === kind)?.label?.trim();
  if (declaredLabel) return declaredLabel;
  return KIND_LABELS[kind] ?? `Note (${kind})`;
}

export interface EventRow {
  key: string;
  time: string;
  kindLabel: string;
  strong: boolean;
  author: string;
  taskName: string | null;
  text: string;
  /** Present only for a collapsed run of same-agent, same-task reports. */
  runLabel: string | null;
}

export type EventLogRow =
  | { key: string; band: string; event: null }
  | { key: string; band: null; event: EventRow };

/** The log prints a task only as a locator, so a long name is clipped, not wrapped. */
export const EVENT_TASK_NAME_MAX = 36;

export function clipTaskName(name: string | null): string | null {
  if (!name) return null;
  return name.length > EVENT_TASK_NAME_MAX ? `${name.slice(0, EVENT_TASK_NAME_MAX)}…` : name;
}

function eventRow(event: PlanEvent, index: number, declared: readonly MilestoneKind[]): EventRow {
  return {
    key: `event-${index}-${event.ts}`,
    time: formatClock(event.ts),
    kindLabel: eventKindLabel(event.kind, declared),
    strong: STRONG_KINDS.has(event.kind),
    author: event.author,
    taskName: event.task_name === event.text ? null : clipTaskName(event.task_name),
    text: event.text,
    runLabel:
      event.count > 1
        ? `×${event.count}, ${formatClock(event.ts)} to ${formatClock(event.last_ts)}`
        : null,
  };
}

/** Time-ordered rows with an hour band wherever the clock hour turns over. */
export function eventLogRows(
  log: EventLog,
  declared: readonly MilestoneKind[] = [],
): EventLogRow[] {
  const rows: EventLogRow[] = [];
  let band: string | null = null;
  log.entries.forEach((event, index) => {
    const hour = formatHourBand(event.ts);
    if (hour !== band) {
      band = hour;
      rows.push({ key: `band-${index}-${hour}`, band: hour, event: null });
    }
    rows.push({ key: `row-${index}`, band: null, event: eventRow(event, index, declared) });
  });
  return rows;
}

export function eventLogCaption(log: EventLog): string {
  const truncated = log.truncated ? ", newest kept" : "";
  return `${formatCount(log.recorded_events)} events · ${formatCount(log.entries.length)} lines · report runs collapsed${truncated}`;
}

export interface AreaRow {
  key: string;
  label: string;
  /** Printed after the label; the fallback names its own source. */
  qualifier: string | null;
  taskCount: number;
  sharePct: number;
}

export interface AreaReading {
  declared: boolean;
  facetLabel: string | null;
  rows: AreaRow[];
  untagged: number;
  /** Set only in the no-facet state; the board prints it verbatim. */
  noFacetMessage: string | null;
}

const NO_FACET_MESSAGE =
  "No area facet declared; stored executor roles are the fallback grouping.";

function share(count: number, total: number): number {
  return total > 0 ? Math.round((count / total) * 100) : 0;
}

export function areaReading(areas: AreaGrouping): AreaReading {
  if (areas.state === "declared") {
    const total =
      areas.groups.reduce((sum, group) => sum + group.task_count, 0) + areas.untagged_task_count;
    return {
      declared: true,
      facetLabel: areas.facet_label ?? areas.facet_key,
      rows: areas.groups.map((group) => ({
        key: group.value,
        label: group.label,
        qualifier: null,
        taskCount: group.task_count,
        sharePct: share(group.task_count, total),
      })),
      untagged: areas.untagged_task_count,
      noFacetMessage: null,
    };
  }
  const total = areas.role_fallback.reduce((sum, group) => sum + group.task_count, 0);
  return {
    declared: false,
    facetLabel: null,
    rows: areas.role_fallback.map((group, index) => ({
      key: group.executor_role ?? `no-role-${index}`,
      label: group.executor_role ?? "No role recorded",
      qualifier: "executor role, not a tag",
      taskCount: group.task_count,
      sharePct: share(group.task_count, total),
    })),
    untagged: 0,
    noFacetMessage: NO_FACET_MESSAGE,
  };
}

export interface RunwayGauge {
  key: string;
  label: string;
  variant: "session" | "weekly";
  pct: number;
  resetsAt: number | null;
  state: UsageState;
  staleReason: string | null;
}

function hasAllowanceWindow(pct: number | null, resetsAt: number | null): pct is number {
  return pct != null && Number.isFinite(pct) && resetsAt != null && Number.isFinite(resetsAt);
}

export function runwayGauges(providers: readonly ProviderRunway[]): RunwayGauge[] {
  const gauges: RunwayGauge[] = [];
  for (const provider of providers) {
    const providerLabel = provider.label.trim() || provider.provider;
    const sessionPct = provider.session_pct;
    const sessionResetsAt = provider.session_resets_at;
    if (hasAllowanceWindow(sessionPct, sessionResetsAt)) {
      gauges.push({
        key: `${provider.provider}-session`,
        label: `${providerLabel} · session allowance used`,
        variant: "session",
        pct: sessionPct,
        resetsAt: sessionResetsAt,
        state: classify(sessionPct),
        staleReason: provider.stale_reason,
      });
    }
    const weeklyPct = provider.weekly_pct;
    const weeklyResetsAt = provider.weekly_resets_at;
    if (hasAllowanceWindow(weeklyPct, weeklyResetsAt)) {
      gauges.push({
        key: `${provider.provider}-weekly`,
        label: `${providerLabel} · weekly allowance used`,
        variant: "weekly",
        pct: weeklyPct,
        resetsAt: weeklyResetsAt,
        state: classify(weeklyPct),
        staleReason: provider.stale_reason,
      });
    }
  }
  return gauges;
}

export const TASK_STATUS_FIELDS = [
  { key: "pending", label: "pending" },
  { key: "in_progress", label: "in progress" },
  { key: "in_review", label: "in review" },
  { key: "blocked", label: "blocked" },
  { key: "paused", label: "paused" },
  { key: "done", label: "done" },
  { key: "cancelled", label: "cancelled" },
] as const;

export function taskStatusBreakdown(counts: PlanTaskCounts) {
  return TASK_STATUS_FIELDS.map((status) => ({
    ...status,
    count: counts[status.key],
  }));
}

const NOT_RECORDING_COPY: Record<KpiNotRecordingReason, string> = {
  disabled: "not recording — the KPI ledger is switched off",
  no_rows_for_plan: "not recording — the ledger holds no row for this plan",
  unreadable: "not recording — the ledger could not be read",
  not_read: "not recording — no ledger reader is attached",
};

export function notRecordingCopy(reason: KpiNotRecordingReason | null): string {
  return reason ? NOT_RECORDING_COPY[reason] : NOT_RECORDING_COPY.not_read;
}

export function kpiEnableCommand(setting: string): string {
  return `codeterm config set ${setting} true`;
}

/**
 * What a person must do about this state. The opt-out branch names the exact setting
 * the snapshot carries, so a not-recording card is never a bare label; every other
 * branch says why no figure exists rather than offering a switch that would not help.
 */
export function notRecordingDetail(
  reason: KpiNotRecordingReason | null,
  enableSetting: string | null,
): string {
  switch (reason ?? "not_read") {
    case "disabled":
      return enableSetting
        ? `Recording off; no figure. Set ${enableSetting}=true, run ${kpiEnableCommand(enableSetting)}, then restart CodeTerm.`
        : "Recording off; no figure. Re-enable [kpi] in config.toml, then restart CodeTerm.";
    case "no_rows_for_plan":
      return "No ledger row for this plan yet; no figure.";
    case "unreadable":
      return "Ledger unreadable; no figure. See the daemon log for the error.";
    default:
      return "No ledger reader attached; no figure.";
  }
}
