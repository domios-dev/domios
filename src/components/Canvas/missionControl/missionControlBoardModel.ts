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
    taskName: clipTaskName(event.task_name),
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
  return `${formatCount(log.recorded_events)} recorded events, ${formatCount(log.entries.length)} lines${truncated}; a run of reports by one agent on one task is one line with a count`;
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
  "This plan declares no area facet, so nothing is grouped by area. These are the executor roles stored on its tasks; the Planner sets an area from the plan's own list and nothing is inferred.";

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

export function runwayGauges(providers: readonly ProviderRunway[]): RunwayGauge[] {
  const gauges: RunwayGauge[] = [];
  for (const provider of providers) {
    if (provider.session_pct != null) {
      gauges.push({
        key: `${provider.provider}-session`,
        label: `${provider.label} session`,
        variant: "session",
        pct: provider.session_pct,
        resetsAt: provider.session_resets_at,
        state: classify(provider.session_pct),
        staleReason: provider.stale_reason,
      });
    }
    if (provider.weekly_pct != null) {
      gauges.push({
        key: `${provider.provider}-weekly`,
        label: `${provider.label} weekly`,
        variant: "weekly",
        pct: provider.weekly_pct,
        resetsAt: provider.weekly_resets_at,
        state: classify(provider.weekly_pct),
        staleReason: provider.stale_reason,
      });
    }
  }
  return gauges;
}

const NOT_RECORDING_COPY: Record<KpiNotRecordingReason, string> = {
  disabled: "not recording — the KPI ledger is off",
  no_rows_for_plan: "not recording — the ledger holds no row for this plan",
  unreadable: "not recording — the ledger could not be read",
  not_read: "not recording — no ledger reader is attached",
};

export function notRecordingCopy(reason: KpiNotRecordingReason | null): string {
  return reason ? NOT_RECORDING_COPY[reason] : NOT_RECORDING_COPY.not_read;
}

export interface ProgressReading {
  done: number;
  active: number;
  inReview: number;
  cancelled: number;
}

export function progressReading(counts: PlanTaskCounts): ProgressReading {
  return {
    done: counts.done,
    active: Math.max(0, counts.total - counts.cancelled),
    inReview: counts.in_review,
    cancelled: counts.cancelled,
  };
}
