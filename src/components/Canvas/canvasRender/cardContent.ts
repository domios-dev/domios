
import { TASK_STATUS_COLORS } from "../settings";
import { flatSections, flattenSections, sectionRows, type SectionRowGlyph } from "../sectionRows";
import { facetColorMap, resolveTags } from "../tagModel";
import type { Plan, Section, Task, TagFacet } from "../types";
import type { TaskPresence } from "../taskPresence";

export type CardLevel = "far" | "near";

export interface CardTag {
  text: string;
  color: string | null;
}

export interface CardSectionLine {
  sectionId: string;
  title: string;
  glyph: SectionRowGlyph;
  tags: CardTag[];
  button: string | null;
  body?: string;
}

export interface CardField {
  label: string;
  value: string;
  color: string | null;
}

export interface CardContent {
  kind: string;
  kicker: string;
  title: string;
  accent: string | null;
  tags: CardTag[];
  sections: CardSectionLine[];
  fields: CardField[];
  /** Readable body for the DEEP band (task description / plan north-star). Markdown source. */
  body?: string;
  hash: string;
}

type PlanModel = Plan & { tag_facets?: TagFacet[] };
type TaskModel = {
  task: Task;
  tagFacets?: TagFacet[];
  childCount?: number;
  childDone?: number;
  presence?: TaskPresence | null;
  historicalExecutor?: import("../taskExecutorHistory").HistoricalExecutorView | null;
};

const KICKER: Record<string, string> = { plan: "PLAN", spec: "SPEC", task: "TASK" };

function chipFor(tag: string, colors: Map<string, string>): CardTag {
  const i = tag.indexOf(":");
  const text = i > 0 ? tag.slice(i + 1).replace(/-/g, " ") : tag.replace(/-/g, " ");
  return { text, color: colors.get(tag) ?? null };
}

function sectionTags(section: Section, colors: Map<string, string>): CardTag[] {
  return (section.tags ?? []).slice(0, 3).map((t) => chipFor(t, colors));
}

// glanceable "what this plan is tagged by" row, capped so it never overflows.
function planTopTags(plan: PlanModel, colors: Map<string, string>): CardTag[] {
  const out: CardTag[] = [];
  for (const facet of plan.tag_facets ?? []) {
    if (!facet.color_lane) continue;
    for (const v of facet.values) {
      const tag = `${facet.key}:${v.v}`;
      out.push({ text: v.label?.trim() || v.v.replace(/-/g, " "), color: colors.get(tag) ?? null });
      if (out.length >= 6) return out;
    }
  }
  return out;
}

function planContent(plan: PlanModel): CardContent {
  const colors = facetColorMap(plan.tag_facets);
  const rowsByGlyph = new Map(sectionRows(plan).map((r) => [r.section.id, r.glyph]));
  const sections: CardSectionLine[] = flatSections(plan).map((s) => ({
    sectionId: s.id,
    title: s.title,
    glyph: rowsByGlyph.get(s.id) ?? "uncovered",
    tags: sectionTags(s, colors),
    button: null,
    body: s.body?.trim() || undefined,
  }));
  const tags = planTopTags(plan, colors);
  return {
    kind: "plan",
    kicker: plan.mode === "paused" ? "PARKED" : KICKER.plan,
    title: plan.title,
    accent: null,
    tags,
    sections,
    fields: [],
    body: plan.goal?.vector?.trim() || undefined,
    hash: sig(["plan", plan.mode, plan.title, tags, sections.map((s) => [s.title, s.glyph, s.tags, s.body ?? ""]), plan.goal?.vector ?? ""]),
  };
}

export interface SpecCoverage {
  total: number;
  covered: number;   // all tasks done/cancelled
  partial: number;   // some tasks exist but not all done
  unplanned: number; // zero tasks
}

export function specCoverage(plan: PlanModel): SpecCoverage {
  const flat = flattenSections(plan.sections);
  const byId = new Map<string, string[]>();
  for (const s of flat) byId.set(s.id, []);
  for (const t of (plan.tasks ?? [])) {
    for (const sid of t.section_ids ?? []) {
      const arr = byId.get(sid);
      if (arr) arr.push(t.status);
    }
  }
  let covered = 0, partial = 0, unplanned = 0;
  for (const statuses of byId.values()) {
    if (statuses.length === 0) unplanned++;
    else if (statuses.every((s) => s === "done" || s === "cancelled")) covered++;
    else partial++;
  }
  return { total: flat.length, covered, partial, unplanned };
}

function sectionGlyph(sectionId: string, plan: PlanModel): SectionRowGlyph {
  const tasks = (plan.tasks ?? []).filter((t) => (t.section_ids ?? []).includes(sectionId));
  if (tasks.length === 0) return "uncovered";
  if (tasks.every((t) => t.status === "done" || t.status === "cancelled")) return "done";
  return "progress";
}

function specContent(plan: PlanModel): CardContent {
  const cov = specCoverage(plan);
  const sections: CardSectionLine[] = flattenSections(plan.sections).map((s) => {
    const glyph = sectionGlyph(s.id, plan);
    // Only add the nav button when the section has tasks to navigate to.
    const taskCount = (plan.tasks ?? []).filter((t) => (t.section_ids ?? []).includes(s.id)).length;
    return {
      sectionId: s.id,
      title: s.title,
      glyph,
      tags: [],
      button: taskCount > 0 ? `${taskCount} task${taskCount > 1 ? "s" : ""}` : null,
      body: s.body?.trim() || undefined,
    };
  });
  // Compact coverage summary chips shown in the tag row.
  const tags: CardTag[] = [];
  if (cov.covered > 0) tags.push({ text: `${cov.covered} done`, color: GLYPH_ACCENT.done });
  if (cov.partial > 0) tags.push({ text: `${cov.partial} active`, color: GLYPH_ACCENT.progress });
  if (cov.unplanned > 0) tags.push({ text: `${cov.unplanned} open`, color: null });

  return {
    kind: "spec",
    kicker: KICKER.spec,
    title: plan.title,
    accent: null,
    tags,
    sections,
    fields: [],
    hash: sig(["spec", plan.title, sections.map((s) => [s.title, s.glyph, s.button ?? "", s.body ?? ""]), tags]),
  };
}

// Exported so drawCard can colour glyph dots consistently with the coverage chips.
export const GLYPH_ACCENT: Record<SectionRowGlyph, string> = {
  done: "rgba(82,180,145,0.95)",
  progress: "rgba(129,140,248,0.95)",
  uncovered: "rgba(148,163,184,0.55)",
};

function executorField(m: TaskModel): CardField {
  const lead = m.presence?.agents?.[0];
  if (lead) {
    return { label: "executor", value: lead.label, color: lead.color };
  }
  if (m.historicalExecutor) {
    return { label: "last executor", value: m.historicalExecutor.summary, color: null };
  }
  return { label: "assignee", value: "—", color: null };
}

function taskContent(m: TaskModel): CardContent {
  const task = m.task;
  const resolved = resolveTags(task.tags, m.tagFacets);
  const tags: CardTag[] = [
    ...resolved.facets.map((f) => ({ text: f.label, color: f.color })),
    ...resolved.topics.map((t) => ({ text: t, color: null })),
  ].slice(0, 5);
  const fields: CardField[] = [
    { label: "status", value: (task.status === "pending" || task.status === "in_progress") && m.childCount != null && m.childCount > (m.childDone ?? 0) ? "blocked on subtasks" : task.status_label?.trim() || task.status.replace(/_/g, " "), color: TASK_STATUS_COLORS[task.status] ?? null },
    { label: "priority", value: (task.priority ?? "p2").toUpperCase(), color: null },
    executorField(m),
  ];
  if (m.presence?.focus) {
    fields.push({ label: "focus", value: m.presence.focus, color: null });
  }
  if (m.childCount && m.childCount > 0) {
    fields.push({ label: "subtasks", value: `${m.childDone ?? 0}/${m.childCount}`, color: null });
  }
  return {
    kind: "task",
    kicker: KICKER.task,
    title: task.name,
    accent: TASK_STATUS_COLORS[task.status] ?? null,
    tags,
    sections: [],
    fields,
    body: task.body?.trim() || undefined,
    hash: sig(["task", task.name, task.status, task.priority ?? "", tags, fields.map((f) => f.value), task.body ?? ""]),
  };
}

// Stable, cheap content signature — drives bitmap-cache invalidation on data change.
function sig(parts: unknown): string {
  return JSON.stringify(parts);
}

interface HandoffData {
  seq?: number;
  goal?: string;
  mode?: string;
  ts?: number;
  next_actions?: string[];
  plan_snapshot?: { coverage_pct?: number };
}

// fields). Migrated to a painter so it stays VISIBLE under the canvas renderer.
function handoffContent(model: unknown): CardContent {
  const c = ((model as { content?: HandoffData }).content ?? {}) as HandoffData;
  const title = c.goal?.trim() || (c.seq != null ? `Handoff #${c.seq}` : "Handoff");
  const tags: CardTag[] = c.mode ? [{ text: c.mode, color: null }] : [];
  const fields: CardField[] = [];
  if (c.plan_snapshot?.coverage_pct != null) fields.push({ label: "coverage", value: `${c.plan_snapshot.coverage_pct}%`, color: null });
  if (c.next_actions?.length) fields.push({ label: "next", value: `${c.next_actions.length} action${c.next_actions.length > 1 ? "s" : ""}`, color: null });
  return { kind: "handoff", kicker: "HANDOFF", title, accent: "rgba(56,189,248,0.92)", tags, sections: [], fields, hash: sig(["handoff", title, c.seq ?? 0, c.ts ?? 0, fields.map((f) => f.value)]) };
}

// Archived-plan lineage ghost → a quiet titled card (kicker ARCHIVED + counts).
function ghostContent(model: unknown): CardContent {
  const g = model as { title?: string; status?: string; sectionCount?: number; taskCount?: number };
  const tags: CardTag[] = [];
  if (g.taskCount) tags.push({ text: `${g.taskCount} tasks`, color: null });
  if (g.sectionCount) tags.push({ text: `${g.sectionCount} sections`, color: null });
  return { kind: "plan-ghost", kicker: "ARCHIVED", title: g.title || "Plan", accent: "rgba(148,163,184,0.65)", tags, sections: [], fields: [], hash: sig(["ghost", g.title, g.status, g.taskCount, g.sectionCount]) };
}

export function buildCardContent(kind: string, model: unknown): CardContent | null {
  if (!model) return null;
  if (kind === "plan") return planContent(model as PlanModel);
  if (kind === "spec") return specContent(model as PlanModel);
  if (kind === "task") return taskContent(model as TaskModel);
  if (kind === "handoff") return handoffContent(model);
  if (kind === "plan-ghost") return ghostContent(model);
  return null;
}

// at the reading font with no scroll (north-star: everything fits). Otherwise FAR.
export interface FitMetrics {
  headerH: number;
  titleLineH: number;
  titleGap: number;
  rowH: number;
  tagRowH: number;
  fieldH: number;
  padY: number;
}

export const FIT: FitMetrics = { headerH: 34, titleLineH: 18, titleGap: 2, rowH: 19, tagRowH: 20, fieldH: 18, padY: 14 };

export interface NearRows {
  titleLines: number;
  hasTags: boolean;
  sections: number;
  fields: number;
}

export function nearContentHeight(rows: NearRows): number {
  return FIT.headerH + rows.titleLines * FIT.titleLineH + FIT.titleGap
    + (rows.hasTags ? FIT.tagRowH : 0)
    + rows.sections * FIT.rowH
    + rows.fields * FIT.fieldH
    + FIT.padY;
}

export const MIN_NEAR_W = 150;
export const MIN_NEAR_H = 96;

export function pickLevel(c: CardContent, screenW: number, screenH: number): CardLevel {
  if (!c.sections.length && !c.fields.length) return "far";
  return screenW >= MIN_NEAR_W && screenH >= MIN_NEAR_H ? "near" : "far";
}
