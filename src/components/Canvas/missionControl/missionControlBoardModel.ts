// Pure reading of the stored Mission Control projection. Nothing here opens a
// session log, parses note prose, or invents a figure the contract does not carry.

import { classify, type UsageState } from "../../../types/usage";
import type { MilestoneKind } from "../types";
import type {
  AreaGrouping,
  EventLog,
  HeldForDecision,
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
  wide: "repeat(4, minmax(0, 1fr))",
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
  return `${formatCount(log.recorded_events)} events · ${formatCount(log.entries.length)} lines`;
}

export interface AreaRow {
  key: string;
  label: string;
  taskCount: number;
  sharePct: number;
}

export interface AreaReading {
  declared: boolean;
  facetLabel: string | null;
  title: "Tasks by area" | "Tasks by role";
  caption: string;
  rows: AreaRow[];
  untagged: number;
  noFacetMessage: string | null;
}

const NO_FACET_MESSAGE =
  "Area facet not declared · using executor roles";

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
      title: "Tasks by area",
      caption: `${areas.facet_label ?? "Area"} facet`,
      rows: areas.groups.map((group) => ({
        key: group.value,
        label: group.label,
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
    title: "Tasks by role",
    caption: "executor role fallback",
    rows: areas.role_fallback.map((group, index) => ({
      key: group.executor_role ?? `no-role-${index}`,
      label: group.executor_role ?? "No role recorded",
      taskCount: group.task_count,
      sharePct: share(group.task_count, total),
    })),
    untagged: 0,
    noFacetMessage: NO_FACET_MESSAGE,
  };
}

export interface DecisionsWaitingReading {
  gateCount: number;
  oldestAgeMs: number | null;
  blockedTaskCount: number;
}

export function decisionsWaitingReading(held: HeldForDecision): DecisionsWaitingReading {
  const oldestAgeMs = held.open_gates.reduce<number | null>(
    (oldest, gate) => (oldest == null || gate.age_ms > oldest ? gate.age_ms : oldest),
    null,
  );
  return {
    gateCount: held.open_gates.length,
    oldestAgeMs,
    blockedTaskCount: held.open_gates.reduce((sum, gate) => sum + gate.blocking_task_count, 0),
  };
}

export interface LongestWaitReading {
  ageMs: number | null;
  taskName: string | null;
}

export function longestWaitReading(held: HeldForDecision): LongestWaitReading {
  const oldest = held.held_reviews.reduce<{ ageMs: number; taskName: string } | null>(
    (candidate, review) => {
      if (review.age_ms == null) return candidate;
      return candidate == null || review.age_ms > candidate.ageMs
        ? { ageMs: review.age_ms, taskName: review.name }
        : candidate;
    },
    null,
  );
  return {
    ageMs: held.oldest_hold_age_ms,
    taskName: oldest && oldest.ageMs === held.oldest_hold_age_ms ? oldest.taskName : null,
  };
}

export interface RunwayGauge {
  key: string;
  label: string;
  variant: "session" | "weekly";
  pct: number | null;
  resetsAt: number | null;
  state: UsageState;
}

function hasAllowanceWindow(pct: number | null, resetsAt: number | null): pct is number {
  return pct != null && Number.isFinite(pct) && resetsAt != null && Number.isFinite(resetsAt);
}

const RUNWAY_READINGS = [
  { provider: "claude", label: "Claude", variant: "session", pct: "session_pct", reset: "session_resets_at" },
  { provider: "claude", label: "Claude", variant: "weekly", pct: "weekly_pct", reset: "weekly_resets_at" },
  { provider: "codex", label: "Codex", variant: "session", pct: "session_pct", reset: "session_resets_at" },
  { provider: "codex", label: "Codex", variant: "weekly", pct: "weekly_pct", reset: "weekly_resets_at" },
] as const;

export function runwayGauges(providers: readonly ProviderRunway[]): RunwayGauge[] {
  return RUNWAY_READINGS.map((reading) => {
    const provider = providers.find((candidate) => candidate.provider.toLowerCase() === reading.provider);
    const pct = provider?.[reading.pct] ?? null;
    const resetsAt = provider?.[reading.reset] ?? null;
    const complete = hasAllowanceWindow(pct, resetsAt);
    return {
      key: `${reading.provider}-${reading.variant}`,
      label: `${provider?.label.trim() || reading.label} · ${reading.variant}`,
      variant: reading.variant,
      pct: complete ? pct : null,
      resetsAt: complete ? resetsAt : null,
      state: classify(complete ? pct : null),
    };
  });
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
  disabled: "not recording",
  no_rows_for_plan: "no plan ledger row",
  unreadable: "ledger unavailable",
  not_read: "ledger not read",
};

export function notRecordingCopy(
  reason: KpiNotRecordingReason | null,
  enableSetting: string | null = null,
): string {
  if (reason === "disabled" && enableSetting) return `recording off · enable ${enableSetting}`;
  return reason ? NOT_RECORDING_COPY[reason] : NOT_RECORDING_COPY.not_read;
}
