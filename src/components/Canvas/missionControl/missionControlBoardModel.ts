// Pure reading of the stored Mission Control projection. Nothing here opens a
// session log, parses note prose, or invents a figure the contract does not carry.

import { shortModelName } from "../../../lib/agentModelName";
import { classify, type UsageState } from "../../../types/usage";
import type { MilestoneKind } from "../types";
import type {
  EventLog,
  HeldForDecision,
  KpiNotRecordingReason,
  MissionControlSpend,
  PlanEvent,
  PlanTaskCounts,
  ProviderSpend,
} from "./missionControlState";

export type BoardLayout = "wide" | "narrow";

/** Below this the board stacks its panels in one column. */
export const BOARD_NARROW_MAX_PX = 720;

export function boardLayout(width: number): BoardLayout {
  return width > 0 && width <= BOARD_NARROW_MAX_PX ? "narrow" : "wide";
}

/** Spend, models and the active plan side by side on a wide board; one column on a phone. */
export const BOARD_ROW_COLUMNS: Record<BoardLayout, string> = {
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

/** A small share keeps one decimal so a real model never rounds away to 0%. */
export function formatSharePct(value: number): string {
  if (!Number.isFinite(value)) return "0%";
  const rounded = value >= 10 ? Math.round(value) : Math.round(value * 10) / 10;
  return `${rounded}%`;
}

export function formatCountOf(count: number, one: string, many: string): string {
  return `${formatCount(count)} ${count === 1 ? one : many}`;
}

export function formatAgeMs(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  if (h > 0) return m > 0 ? `${h} h ${String(m).padStart(2, "0")} m` : `${h} h`;
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
  /** The event's task, so the row can name it by its key and open it. */
  taskId: string | null;
  taskName: string | null;
  /** Bounded single-line reading of `body`; never the only copy of the text. */
  preview: string;
  /** The stored note exactly as recorded, reachable through the row's reveal. */
  body: string;
  expandable: boolean;
  /** Present only for a collapsed run of same-agent, same-task reports. */
  runLabel: string | null;
}

export type EventLogRow =
  | { key: string; band: string; event: null }
  | { key: string; band: null; event: EventRow };

/** The log prints a task only as a locator, so a long name is clipped, not wrapped. */
export const EVENT_TASK_NAME_MAX = 36;

/** Punctuation the row prints between a clipped task name and the note itself. */
export const EVENT_PART_SEPARATOR = " · ";

/** The log is an index: a decision or question is read here, not consumed here. */
export const EVENT_PREVIEW_MAX = 100;

export const EVENT_REVEAL_SHOW_LABEL = "Show full event text";
export const EVENT_REVEAL_HIDE_LABEL = "Hide full event text";

export function clipTaskName(name: string | null): string | null {
  if (!name) return null;
  return name.length > EVENT_TASK_NAME_MAX ? `${name.slice(0, EVENT_TASK_NAME_MAX)}…` : name;
}

/** A word-boundary cut of one flattened line, so the preview never ends mid-token. */
export function eventPreview(body: string): string {
  const flat = body.replace(/\s+/g, " ").trim();
  if (flat.length <= EVENT_PREVIEW_MAX) return flat;
  const cut = flat.slice(0, EVENT_PREVIEW_MAX);
  const lastSpace = cut.lastIndexOf(" ");
  const head = lastSpace > EVENT_PREVIEW_MAX / 2 ? cut.slice(0, lastSpace) : cut;
  return `${head.replace(/[\s,;:.–—-]+$/, "")}…`;
}

function eventRow(event: PlanEvent, index: number, declared: readonly MilestoneKind[]): EventRow {
  const preview = eventPreview(event.text);
  return {
    key: `event-${event.ts}-${event.kind}-${index}`,
    time: formatClock(event.ts),
    kindLabel: eventKindLabel(event.kind, declared),
    strong: STRONG_KINDS.has(event.kind),
    author: event.author,
    taskId: event.task_id,
    taskName: event.task_name === event.text ? null : clipTaskName(event.task_name),
    preview,
    body: event.text,
    expandable: preview !== event.text,
    runLabel:
      event.count > 1
        ? `×${event.count}, ${formatClock(event.ts)} to ${formatClock(event.last_ts)}`
        : null,
  };
}

/** Rows in the order given, with an hour band wherever the clock hour turns over. */
export function eventLogRows(
  log: EventLog,
  declared: readonly MilestoneKind[] = [],
  indexOffset = 0,
): EventLogRow[] {
  const rows: EventLogRow[] = [];
  let band: string | null = null;
  log.entries.forEach((entry, position) => {
    const index = indexOffset + position;
    const hour = formatHourBand(entry.ts);
    if (hour !== band) {
      band = hour;
      rows.push({ key: `band-${index}-${hour}`, band: hour, event: null });
    }
    rows.push({ key: `row-${index}`, band: null, event: eventRow(entry, index, declared) });
  });
  return rows;
}

/** Events per page of the log. */
export const EVENT_LOG_PAGE = 50;

/** Page numbers for the pager: all of them when few, otherwise the ends and the neighbours of the current page. */
export function pagerPages(current: number, count: number): Array<number | "gap"> {
  if (count <= 7) return Array.from({ length: count }, (_, index) => index + 1);
  const shown = new Set([1, count, current - 1, current, current + 1].filter((page) => page >= 1 && page <= count));
  const pages: Array<number | "gap"> = [];
  let previous = 0;
  for (const page of [...shown].sort((a, b) => a - b)) {
    if (page - previous > 1) pages.push("gap");
    pages.push(page);
    previous = page;
  }
  return pages;
}

/** Where the log reads its pages from: the plan's stored log, or the team's cursor-paged event stream. */
export interface EventPager {
  /** Pages known so far, including one more when the source says older events exist. */
  pageCount: number;
  /** One page of events, newest first; null while that page is not loaded yet. */
  page(index: number): PlanEvent[] | null;
  /** Asks the source to fetch a page it does not hold yet. */
  load?(index: number): void;
  /** Short count for the log header. */
  caption: string;
}

export function planEventPager(log: EventLog): EventPager {
  const newest = [...log.entries].reverse();
  return {
    pageCount: Math.max(1, Math.ceil(newest.length / EVENT_LOG_PAGE)),
    page: (index) => newest.slice(index * EVENT_LOG_PAGE, (index + 1) * EVENT_LOG_PAGE),
    caption: eventLogCaption(log),
  };
}

const REF_PATTERN = /\b(?:pl|task)_[a-z0-9]+\b/g;

export type RefPart = { kind: "text"; text: string } | { kind: "ref"; id: string };

/** Splits stored text around plan and task ids so each can be shown by its title. */
export function splitRefs(text: string): RefPart[] {
  const parts: RefPart[] = [];
  let last = 0;
  for (const match of text.matchAll(REF_PATTERN)) {
    const at = match.index ?? 0;
    if (at > last) parts.push({ kind: "text", text: text.slice(last, at) });
    parts.push({ kind: "ref", id: match[0] });
    last = at + match[0].length;
  }
  if (last < text.length) parts.push({ kind: "text", text: text.slice(last) });
  return parts;
}

export function eventLogCaption(log: EventLog): string {
  const events = formatCountOf(log.recorded_events, "event", "events");
  return log.truncated ? `latest of ${events}` : events;
}

/** One thing waiting on the owner: an open decision gate or a review held for a decision. */
export interface NeedsYouItem {
  key: string;
  kind: "gate" | "review";
  title: string;
  /** The held review's task; gates name no single task. */
  taskId: string | null;
  facts: string[];
  /** The tab a gate row jumps to; reviews have none. */
  tabId: string | null;
}

export const GATE_NO_TAB_COPY = "unavailable: no tab asked or answers this gate";

export function needsYouItems(held: HeldForDecision): NeedsYouItem[] {
  const gates = held.open_gates.map<NeedsYouItem>((gate) => ({
    key: `gate-${gate.id}`,
    kind: "gate",
    taskId: null,
    tabId: gate.tab_id,
    title: eventPreview(gate.question),
    facts: [
      "decision gate",
      `waiting ${formatAgeMs(gate.age_ms)}`,
      ...(gate.blocking_task_count > 0
        ? [`blocks ${formatCountOf(gate.blocking_task_count, "task", "tasks")}`]
        : []),
    ],
  }));
  const reviews = held.held_reviews.map<NeedsYouItem>((review) => ({
    key: `review-${review.task_id}`,
    kind: "review",
    taskId: review.task_id,
    tabId: null,
    title: review.name,
    facts: [
      "review held",
      ...(review.age_ms == null ? [] : [`waiting ${formatAgeMs(review.age_ms)}`]),
      formatCountOf(review.verdicts, "verdict", "verdicts"),
    ],
  }));
  return [...gates, ...reviews];
}

/** One provider allowance window already consumed. Never a per-model figure. */
export interface SpendWindow {
  key: string;
  variant: "weekly" | "session";
  pct: number;
  state: UsageState;
}

export const SPEND_WINDOW_LABELS: Record<SpendWindow["variant"], string> = {
  weekly: "week",
  session: "session",
};

export interface SpendProviderGroup {
  key: string;
  label: string;
  /** Position among providers; picks the provider's swatch colour. */
  tone: number;
  windows: SpendWindow[];
}

export interface SpendReading {
  recording: boolean;
  /** Why the ledger is not measuring; null while it is. */
  note: string | null;
  /** Replaces the readings when there is nothing at all to show; never a dash. */
  emptyCopy: string | null;
  groups: SpendProviderGroup[];
}

/** One model's new input, the spend that costs, and its share of all new input. */
export interface ModelSpendRow {
  key: string;
  label: string;
  unassigned: boolean;
  tone: number;
  sharePct: number;
  newInput: number;
}

/** Models the card shows before its Show all control. */
export const MODELS_PREVIEW = 5;

export function modelSpendRows(spend: MissionControlSpend): ModelSpendRow[] {
  return spend.providers
    .flatMap((provider, tone) =>
      provider.models.map((model, index) => ({
        key: `${provider.provider}-${model.model ?? `unassigned-${index}`}`,
        label: model.model == null ? UNASSIGNED_MODEL_LABEL : shortModelName(model.model) ?? model.model,
        unassigned: model.model == null,
        tone,
        sharePct: model.token_share_pct,
        newInput: model.uncached_input_tokens,
      })),
    )
    .sort((a, b) => b.newInput - a.newInput);
}

/** The row label for a ledger model the provider never resolved. */
export const UNASSIGNED_MODEL_LABEL = "Unassigned model";

export const SPEND_SCOPE_CAPTION = "this machine";

function hasAllowanceWindow(pct: number | null, resetsAt: number | null): pct is number {
  return pct != null && Number.isFinite(pct) && resetsAt != null && Number.isFinite(resetsAt);
}

const SPEND_WINDOWS = [
  { variant: "weekly", pct: "weekly_used_pct", reset: "weekly_resets_at" },
  { variant: "session", pct: "session_used_pct", reset: "session_resets_at" },
] as const;

/** Only complete windows are read; an incomplete one is left out rather than guessed. */
function providerWindows(provider: ProviderSpend): SpendWindow[] {
  return SPEND_WINDOWS.flatMap((reading) => {
    const pct = provider.allowance?.[reading.pct] ?? null;
    const resetsAt = provider.allowance?.[reading.reset] ?? null;
    if (!hasAllowanceWindow(pct, resetsAt)) return [];
    return [{ key: `${provider.provider}-${reading.variant}`, variant: reading.variant, pct, state: classify(pct) }];
  });
}

/** Machine scope has no plan to miss: an empty ledger is simply no data. */
const SPEND_NOT_RECORDING_COPY: Record<KpiNotRecordingReason, string> = {
  disabled: "not recording",
  no_rows_for_plan: "no data",
  unreadable: "ledger unavailable",
  not_read: "ledger not read",
};

export function spendNotRecordingCopy(
  reason: KpiNotRecordingReason | null,
  enableSetting: string | null = null,
): string {
  if (reason === "disabled" && enableSetting) return `not recording, enable ${enableSetting}`;
  return reason ? SPEND_NOT_RECORDING_COPY[reason] : SPEND_NOT_RECORDING_COPY.not_read;
}

export function spendReading(spend: MissionControlSpend): SpendReading {
  const recording = spend.state === "recording";
  const note = recording
    ? null
    : spendNotRecordingCopy(spend.not_recording_reason, spend.enable_setting);
  const groups = spend.providers.map<SpendProviderGroup>((provider, tone) => ({
    key: provider.provider || "unassigned-provider",
    label: provider.label.trim() || provider.provider,
    tone,
    windows: providerWindows(provider),
  }));
  return {
    recording,
    note,
    emptyCopy: groups.length === 0 ? note ?? SPEND_NOT_RECORDING_COPY.no_rows_for_plan : null,
    groups,
  };
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

export type TaskStatusKey = (typeof TASK_STATUS_FIELDS)[number]["key"];

export interface ProgressReading {
  total: number;
  /** Statuses holding at least one task, in lifecycle order; empty statuses are not shown. */
  active: Array<{ key: TaskStatusKey; label: string; count: number }>;
}

export function progressReading(counts: PlanTaskCounts): ProgressReading {
  return {
    total: counts.total,
    active: TASK_STATUS_FIELDS.map((status) => ({ ...status, count: counts[status.key] })).filter(
      (status) => status.count > 0,
    ),
  };
}

/** A plan's goal statement when it has one; the title always names the plan. */
export function planGoal(plan: { goal?: { vector: string } | null }): string | null {
  return plan.goal?.vector?.trim() || null;
}

export function formatPlanDate(ms: number): string {
  return new Date(ms).toLocaleDateString("en-US", { month: "short", day: "numeric" });
}
