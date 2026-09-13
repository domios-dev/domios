// Pure reading of the stored Mission Control projection. Nothing here opens a
// session log, parses note prose, or invents a figure the contract does not carry.

import { shortModelName } from "../../../lib/agentModelName";
import { classify, type UsageState } from "../../../types/usage";
import type { MilestoneKind } from "../types";
import type {
  AreaGrouping,
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

/** Progress beside Spend on a wide board; one column on a phone. */
export const BOARD_ROW_COLUMNS: Record<BoardLayout, string> = {
  wide: "minmax(0, 1fr) minmax(0, 2fr)",
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
    key: `event-${index}-${event.ts}`,
    time: formatClock(event.ts),
    kindLabel: eventKindLabel(event.kind, declared),
    strong: STRONG_KINDS.has(event.kind),
    author: event.author,
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
  const events = formatCountOf(log.recorded_events, "event", "events");
  return log.truncated ? `latest of ${events}` : events;
}

export interface AreaRow {
  key: string;
  label: string;
  taskCount: number;
  untagged: boolean;
}

export interface AreaReading {
  title: string;
  rows: AreaRow[];
}

export const ROLE_FALLBACK_TITLE = "Roles";

export function areaReading(areas: AreaGrouping): AreaReading {
  if (areas.state === "declared") {
    const rows: AreaRow[] = areas.groups.map((group) => ({
      key: group.value,
      label: group.label,
      taskCount: group.task_count,
      untagged: false,
    }));
    if (areas.untagged_task_count > 0) {
      rows.push({ key: "untagged", label: "untagged", taskCount: areas.untagged_task_count, untagged: true });
    }
    return { title: areas.facet_label ?? areas.facet_key ?? "Areas", rows };
  }
  return {
    title: ROLE_FALLBACK_TITLE,
    rows: areas.role_fallback.map((group, index) => ({
      key: group.executor_role ?? `no-role-${index}`,
      label: group.executor_role ?? "No role recorded",
      taskCount: group.task_count,
      untagged: false,
    })),
  };
}

/** One thing waiting on the owner: an open decision gate or a review held for a decision. */
export interface NeedsYouItem {
  key: string;
  kind: "gate" | "review";
  title: string;
  facts: string[];
}

export function needsYouItems(held: HeldForDecision): NeedsYouItem[] {
  const gates = held.open_gates.map<NeedsYouItem>((gate) => ({
    key: `gate-${gate.id}`,
    kind: "gate",
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

/** Token figures in plain words; read from cache is the input not billed as new. */
export interface SpendTokens {
  newInput: number;
  fromCache: number;
  generated: number;
  turns: number;
}

export const SPEND_TOKEN_WORDS: ReadonlyArray<{
  key: keyof SpendTokens;
  word: string;
  one: string;
  compact: boolean;
}> = [
  { key: "newInput", word: "new input", one: "new input", compact: true },
  { key: "fromCache", word: "read from cache", one: "read from cache", compact: true },
  { key: "generated", word: "generated", one: "generated", compact: true },
  { key: "turns", word: "turns", one: "turn", compact: false },
];

export const SPEND_WINDOW_WORDS: Record<SpendWindow["variant"], string> = {
  weekly: "of week used",
  session: "of session used",
};

/** Allowance points spent per provider need a usage-snapshot projection the state does not carry yet. */
export const SPEND_ALLOWANCE_SPENT_NEEDS = "needs projection change";
/** The ledger records no tool calls yet. */
export const SPEND_TOOL_CALLS_NEEDS = "needs new data";

export interface SpendModelRow {
  key: string;
  label: string;
  unassigned: boolean;
  /** Position of the owning provider, so a model shares its provider's swatch. */
  tone: number;
  sharePct: number;
  tokens: SpendTokens;
}

export interface SpendProviderGroup {
  key: string;
  label: string;
  tone: number;
  windows: SpendWindow[];
  measured: boolean;
  tokens: SpendTokens;
  models: SpendModelRow[];
}

export interface SpendReading {
  recording: boolean;
  /** Why the ledger is not measuring; null while it is. */
  note: string | null;
  /** Replaces the steps when there is nothing at all to show; never a dash. */
  emptyCopy: string | null;
  groups: SpendProviderGroup[];
  models: SpendModelRow[];
}

export function spendTokens(source: {
  turns: number;
  input_tokens: number;
  uncached_input_tokens: number;
  output_tokens: number;
}): SpendTokens {
  return {
    newInput: source.uncached_input_tokens,
    fromCache: Math.max(0, source.input_tokens - source.uncached_input_tokens),
    generated: source.output_tokens,
    turns: source.turns,
  };
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

function modelRows(provider: ProviderSpend, tone: number): SpendModelRow[] {
  return provider.models.map((model, index) => ({
    key: `${provider.provider}-${model.model ?? `unassigned-${index}`}`,
    label: model.model == null ? UNASSIGNED_MODEL_LABEL : shortModelName(model.model) ?? model.model,
    unassigned: model.model == null,
    tone,
    sharePct: model.token_share_pct,
    tokens: spendTokens(model),
  }));
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
    measured: provider.models.length > 0,
    tokens: spendTokens(provider),
    models: modelRows(provider, tone),
  }));
  return {
    recording,
    note,
    emptyCopy: groups.length === 0 ? note ?? SPEND_NOT_RECORDING_COPY.no_rows_for_plan : null,
    groups,
    models: groups.flatMap((group) => group.models),
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
  if (reason === "disabled" && enableSetting) return `recording off, enable ${enableSetting}`;
  return reason ? NOT_RECORDING_COPY[reason] : NOT_RECORDING_COPY.not_read;
}
