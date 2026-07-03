import { RichMarkdown } from "@codeterm/chat-ui";
import type { CSSProperties, ReactNode } from "react";
import type { Pane } from "../../../lib/types";
import { historicalExecutorForTask } from "../taskExecutorHistory";
import { TASK_STATUS_COLORS } from "../settings";
import type { Plan, Section, Task } from "../types";
import type { SectionNavArgs } from "../nodeRegistry";
import type { PlanDetailKind } from "../planView/PlanDetailOverlay";
import { GLYPH_ACCENT, specCoverage } from "./cardContent";

// The READ surface body for the detail overlay. Plain (no section tint) — structure
// + rendered markdown only. Task: description + structured fields. Plan/spec: progress
// (plan) + the SECTIONS as the hero, each body rendered as markdown; meta is a compact
// secondary strip, never the headline.

export interface DetailContentProps {
  kind: PlanDetailKind;
  planId?: string;
  task?: Task;
  plan?: Plan;
  spec?: Plan;
  specSections?: Section[];
  panes?: Record<string, Pane>;
  onOpenPane?: (paneId: string) => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
  renderTaskAttachmentThumbnails?: (task: Task) => ReactNode;
}

// Natural-height content; the overlay BODY wrapper is the scroll container (overflow-y
// auto + min-height:0). The content must NOT be height:100% — it has to grow past the
// body so the body actually scrolls (the iter3 centering trap clipped it otherwise).
const wrap: CSSProperties = {
  padding: "20px 24px 28px",
  display: "flex",
  flexDirection: "column",
  alignItems: "stretch",
  gap: 18,
  fontFamily: "var(--ct-font-ui)",
  color: "var(--ct-fg)",
  background: "transparent",
};
const labelStyle: CSSProperties = { fontSize: 11, fontWeight: 700, letterSpacing: "0.07em", textTransform: "uppercase", color: "color-mix(in srgb, var(--ct-fg) 52%, transparent)" };
const sectionTitle: CSSProperties = { fontSize: 15, fontWeight: 700, color: "var(--ct-fg)" };
const muted: CSSProperties = { fontSize: 13, fontStyle: "italic", color: "color-mix(in srgb, var(--ct-fg) 50%, transparent)" };

function Chip({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 6, padding: "3px 9px", borderRadius: 999, border: "1px solid var(--ct-border-subtle)", fontSize: 12, background: "color-mix(in srgb, var(--ct-fg) 5%, transparent)" }}>
      <span style={{ ...labelStyle, fontSize: 10 }}>{label}</span>
      <span style={{ display: "inline-flex", alignItems: "center", gap: 5, fontWeight: 600 }}>
        {color && <span style={{ width: 7, height: 7, borderRadius: "50%", background: color }} />}
        {value}
      </span>
    </span>
  );
}

function Block({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <div style={labelStyle}>{label}</div>
      {children}
    </div>
  );
}

function Markdown({ text }: { text: string }) {
  return (
    <div data-canvas-scroll-body="1" style={{ fontSize: 13.5, lineHeight: 1.55, color: "var(--ct-fg)" }}>
      <RichMarkdown text={text} />
    </div>
  );
}

function flattenSections(sections: Section[]): Section[] {
  return sections.flatMap((section) => [section, ...flattenSections(section.subsections ?? [])]);
}

function linkedTaskSections(task: Task, plan?: Plan): Section[] {
  if (!plan || task.section_ids.length === 0) return [];
  const byId = new Map(flattenSections(plan.sections).map((section) => [section.id, section]));
  return task.section_ids.map((id) => byId.get(id)).filter((section): section is Section => Boolean(section));
}

function TaskSectionLinks({
  task,
  plan,
  planId,
  onSection,
}: {
  task: Task;
  plan?: Plan;
  planId?: string;
  onSection?: (a: SectionNavArgs) => void;
}) {
  const sections = linkedTaskSections(task, plan);
  if (sections.length === 0) return null;
  const navPlanId = planId ?? plan?.id;
  return (
    <Block label="Plan sections">
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
        {sections.map((section) => (
          <button
            key={section.id}
            type="button"
            data-testid="plan-detail-task-section-link"
            data-section-id={section.id}
            onClick={() => navPlanId && onSection?.({ planId: navPlanId, sectionId: section.id, variant: "plan" })}
            style={{
              padding: "5px 9px",
              borderRadius: "var(--ct-radius-sm)",
              border: "1px solid var(--ct-border-subtle)",
              background: "color-mix(in srgb, var(--ct-accent) 9%, transparent)",
              color: "var(--ct-accent)",
              fontSize: 12,
              fontWeight: 650,
              cursor: onSection && navPlanId ? "pointer" : "default",
              textAlign: "left",
            }}
          >
            {section.title}
          </button>
        ))}
      </div>
    </Block>
  );
}

function TaskDetail({
  task,
  plan,
  planId,
  panes = {},
  onOpenPane,
  onSection,
  renderTaskAttachmentThumbnails,
}: {
  task: Task;
  plan?: Plan;
  planId?: string;
  panes?: Record<string, Pane>;
  onOpenPane?: (paneId: string) => void;
  onSection?: (a: SectionNavArgs) => void;
  renderTaskAttachmentThumbnails?: (task: Task) => ReactNode;
}) {
  const body = task.body?.trim();
  const historical = historicalExecutorForTask(task, panes, false);
  const paneId = task.assigned_pane_id?.trim() || null;
  const attachmentThumbnails = renderTaskAttachmentThumbnails?.(task);
  return (
    <div style={wrap} data-testid="canvas-detail-content">
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center" }}>
        <Chip label="status" value={task.status_label?.trim() || task.status.replace(/_/g, " ")} color={TASK_STATUS_COLORS[task.status]} />
        {task.priority && <Chip label="priority" value={task.priority.toUpperCase()} />}
        {task.type && <Chip label="type" value={task.type} />}
        {onOpenPane && paneId && (
          <button
            type="button"
            data-testid="plan-detail-open-pane"
            onClick={() => onOpenPane(paneId)}
            style={{ marginLeft: "auto", padding: "4px 10px", borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-accent)", fontSize: 12, fontWeight: 600, cursor: "pointer" }}
          >
            Show agent
          </button>
        )}
      </div>
      <TaskSectionLinks task={task} plan={plan} planId={planId} onSection={onSection} />
      {attachmentThumbnails && (
        <Block label="Attachments">
          <div data-testid="plan-detail-attachment-thumbnails">{attachmentThumbnails}</div>
        </Block>
      )}
      {historical && (
        <Block label="Execution history">
          <div data-testid="canvas-detail-executor-history" style={{ display: "flex", flexDirection: "column", gap: 8, fontSize: 13 }}>
            {historical.attempts.map((attempt, index) => {
              const label = attempt.pane_title?.trim()
                || (attempt.pane_id ? (panes[attempt.pane_id]?.title?.trim() || attempt.pane_id.slice(0, 8)) : "unknown");
              const bits = [label, attempt.role, attempt.model, attempt.branch, attempt.report_status?.replace(/_/g, " ")]
                .map((v) => v?.trim())
                .filter(Boolean);
              return (
                <div
                  key={attempt.id ?? `${attempt.pane_id ?? "attempt"}-${index}`}
                  data-testid="canvas-detail-executor-attempt"
                  data-executor-attempt={index + 1}
                  style={{ color: "color-mix(in srgb, var(--ct-fg) 78%, transparent)" }}
                >
                  {bits.join(" · ")}
                </div>
              );
            })}
          </div>
        </Block>
      )}
      <Block label="Description">
        {body ? <Markdown text={body} /> : <span style={muted} data-testid="canvas-detail-empty-body">No description</span>}
      </Block>
      {task.acceptance_criteria?.length ? (
        <Block label="Acceptance criteria">
          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 13.5, lineHeight: 1.5, display: "flex", flexDirection: "column", gap: 3 }}>
            {task.acceptance_criteria.map((c, i) => <li key={i}>{c}</li>)}
          </ul>
        </Block>
      ) : null}
      {task.test_plan?.trim() && <Block label="Test plan"><Markdown text={task.test_plan} /></Block>}
      {task.correctness?.trim() && <Block label="Correctness"><Markdown text={task.correctness} /></Block>}
      {task.links?.length ? (
        <Block label="Links">
          <div style={{ display: "flex", flexDirection: "column", gap: 3, fontSize: 13 }}>
            {task.links.map((l, i) => <span key={i} style={{ color: "var(--ct-accent)", wordBreak: "break-all" }}>{l}</span>)}
          </div>
        </Block>
      ) : null}
      {task.milestones.length ? (
        <Block label={`Strides (${task.milestones.length})`}>
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            {task.milestones.map((m) => (
              <div key={m.id} style={{ fontSize: 13, lineHeight: 1.45 }}>
                <span style={{ ...labelStyle, fontSize: 10, marginRight: 6 }}>{m.kind.replace(/[_-]+/g, " ")}</span>
                {m.text}
              </div>
            ))}
          </div>
        </Block>
      ) : null}
    </div>
  );
}

function SectionCoverageGlyph({ glyph }: { glyph: "done" | "progress" | "uncovered" }) {
  const color = GLYPH_ACCENT[glyph];
  const title = glyph === "done" ? "All tasks done" : glyph === "progress" ? "Tasks in progress" : "No tasks yet";
  return (
    <span
      title={title}
      style={{ display: "inline-block", width: 8, height: 8, borderRadius: "50%", background: color, flexShrink: 0, marginTop: 2 }}
    />
  );
}

function sectionTaskGlyph(sectionId: string, tasks: Task[]): "done" | "progress" | "uncovered" {
  const linked = tasks.filter((t) => (t.section_ids ?? []).includes(sectionId));
  if (linked.length === 0) return "uncovered";
  if (linked.every((t) => t.status === "done" || t.status === "cancelled")) return "done";
  return "progress";
}

function SectionTree({ sections, depth = 0, onSection, tasks }: { sections: Section[]; depth?: number; onSection?: (sectionId: string) => void; tasks?: Task[] }) {
  return (
    <>
      {sections.map((s) => {
        const linkedTasks = tasks ? tasks.filter((t) => (t.section_ids ?? []).includes(s.id)) : [];
        const glyph = tasks ? sectionTaskGlyph(s.id, tasks) : "uncovered";
        const taskCount = linkedTasks.length;
        return (
          <div key={s.id} data-testid="canvas-detail-section" data-section-id={s.id} style={{ display: "flex", flexDirection: "column", gap: 6, marginLeft: depth * 16, paddingTop: depth ? 4 : 0 }}>
            <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
              {tasks && <SectionCoverageGlyph glyph={glyph} />}
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ display: "flex", alignItems: "baseline", gap: 8, flexWrap: "wrap" }}>
                  <button
                    type="button"
                    data-testid="canvas-detail-section-nav"
                    title={taskCount > 0 ? `Go to ${taskCount} task${taskCount > 1 ? "s" : ""} in this section` : "No tasks in this section"}
                    onClick={() => onSection?.(s.id)}
                    style={{ ...sectionTitle, fontSize: depth ? 13.5 : 15, display: "inline", padding: 0, border: "none", background: "transparent", color: "var(--ct-fg)", cursor: onSection ? "pointer" : "default", textAlign: "left" }}
                  >
                    {s.title}
                  </button>
                  {taskCount > 0 && (
                    <span
                      data-testid="canvas-detail-section-task-count"
                      style={{ fontSize: 11, fontWeight: 600, color: GLYPH_ACCENT[glyph], flexShrink: 0 }}
                    >
                      {taskCount} task{taskCount > 1 ? "s" : ""}
                    </span>
                  )}
                </div>
              </div>
            </div>
            {s.tags?.length ? (
              <div style={{ display: "flex", flexWrap: "wrap", gap: 5, marginLeft: tasks ? 16 : 0 }}>
                {s.tags.map((t) => <span key={t} style={{ fontSize: 11, padding: "1px 7px", borderRadius: 999, background: "color-mix(in srgb, var(--ct-fg) 7%, transparent)" }}>{t.includes(":") ? t.slice(t.indexOf(":") + 1).replace(/-/g, " ") : t}</span>)}
              </div>
            ) : null}
            {s.body?.trim() ? <div style={{ marginLeft: tasks ? 16 : 0 }}><Markdown text={s.body} /></div> : null}
            {s.subsections?.length ? <SectionTree sections={s.subsections} depth={depth + 1} onSection={onSection} tasks={tasks} /> : null}
          </div>
        );
      })}
    </>
  );
}

function SpecCoverageHeader({ plan }: { plan: Plan }) {
  const cov = specCoverage(plan as Parameters<typeof specCoverage>[0]);
  if (cov.total === 0) return null;
  const pct = Math.round(((cov.covered + cov.partial) / cov.total) * 100);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }} data-testid="canvas-detail-spec-coverage">
      <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
        <span style={{ fontSize: 28, fontWeight: 800, lineHeight: 1 }}>{cov.total}</span>
        <span style={{ fontSize: 13, color: "color-mix(in srgb, var(--ct-fg) 60%, transparent)" }}>
          section{cov.total !== 1 ? "s" : ""}
        </span>
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
        {cov.covered > 0 && (
          <span data-testid="canvas-detail-spec-covered" style={{ display: "inline-flex", alignItems: "center", gap: 5, fontSize: 12, fontWeight: 600 }}>
            <span style={{ width: 8, height: 8, borderRadius: "50%", background: GLYPH_ACCENT.done }} />
            {cov.covered} done
          </span>
        )}
        {cov.partial > 0 && (
          <span data-testid="canvas-detail-spec-partial" style={{ display: "inline-flex", alignItems: "center", gap: 5, fontSize: 12, fontWeight: 600 }}>
            <span style={{ width: 8, height: 8, borderRadius: "50%", background: GLYPH_ACCENT.progress }} />
            {cov.partial} active
          </span>
        )}
        {cov.unplanned > 0 && (
          <span data-testid="canvas-detail-spec-unplanned" style={{ display: "inline-flex", alignItems: "center", gap: 5, fontSize: 12, fontWeight: 600, color: "color-mix(in srgb, var(--ct-fg) 55%, transparent)" }}>
            <span style={{ width: 8, height: 8, borderRadius: "50%", background: GLYPH_ACCENT.uncovered }} />
            {cov.unplanned} unplanned
          </span>
        )}
        {cov.total > 0 && (
          <span style={{ fontSize: 12, color: "color-mix(in srgb, var(--ct-fg) 45%, transparent)" }}>{pct}% covered</span>
        )}
      </div>
      <div style={{ height: 5, borderRadius: 999, background: "color-mix(in srgb, var(--ct-fg) 10%, transparent)", overflow: "hidden" }}>
        <div style={{ width: `${Math.round((cov.covered / cov.total) * 100)}%`, height: "100%", background: GLYPH_ACCENT.done }} />
      </div>
    </div>
  );
}

function PlanDetail({ plan, variant, onSection }: { plan: Plan; variant: "plan" | "spec"; onSection?: (sectionId: string) => void }) {
  const sections = plan.sections;
  const total = plan.tasks.length;
  const done = plan.tasks.filter((t) => t.status === "done" || t.status === "cancelled").length;
  const pct = total ? Math.round((done / total) * 100) : 0;
  const metaBits = [
    plan.status === "active" ? "Active" : plan.status,
    plan.owner.title || plan.owner.pane_id ? `owner ${plan.owner.title ?? plan.owner.pane_id}` : null,
    plan.primary_spec_id ? "linked spec" : null,
    plan.loops?.length ? `${plan.loops.length} loop${plan.loops.length > 1 ? "s" : ""}` : null,
  ].filter(Boolean) as string[];

  return (
    <div style={wrap} data-testid="canvas-detail-content">
      {variant === "plan" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {total > 0 && (
            <div style={{ display: "flex", alignItems: "center", gap: 10 }} data-testid="canvas-detail-progress">
              <span style={{ fontSize: 12, fontWeight: 650, color: "color-mix(in srgb, var(--ct-fg) 68%, transparent)" }}>
                {done} / {total} tasks done
              </span>
              <div style={{ width: 92, height: 5, borderRadius: 999, background: "color-mix(in srgb, var(--ct-fg) 10%, transparent)", overflow: "hidden" }}>
                <div style={{ width: `${pct}%`, height: "100%", background: "var(--ct-accent)" }} />
              </div>
            </div>
          )}
          {plan.goal?.vector && <div style={{ fontSize: 13.5, lineHeight: 1.5, color: "var(--ct-fg)" }}><span style={{ ...labelStyle, marginRight: 8 }}>Goal</span>{plan.goal.vector}</div>}
          {metaBits.length > 0 && <div style={{ fontSize: 12, color: "color-mix(in srgb, var(--ct-fg) 55%, transparent)" }}>{metaBits.join("  ·  ")}</div>}
        </div>
      )}
      {variant === "spec" && <SpecCoverageHeader plan={plan} />}
      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <div style={labelStyle}>{variant === "spec" ? "Specification" : "Sections"}</div>
        {sections.length
          ? <SectionTree sections={sections} onSection={onSection} tasks={variant === "spec" ? plan.tasks : undefined} />
          : <span style={muted}>No sections.</span>}
      </div>
    </div>
  );
}

function resolvePlanModel(kind: PlanDetailKind, plan?: Plan, spec?: Plan, specSections?: Section[]): Plan | null {
  if (kind === "plan") return plan ?? null;
  if (kind !== "spec") return null;
  if (!spec) return null;
  if (specSections) return { ...spec, sections: specSections, tasks: spec.tasks ?? [] };
  return spec;
}

export function DetailContent({ kind, planId, task, plan, spec, specSections, panes, onOpenPane, onSectionClick, onSectionFrame, renderTaskAttachmentThumbnails }: DetailContentProps) {
  if (kind === "task") {
    const onSection = onSectionFrame ?? onSectionClick;
    return task ? (
      <TaskDetail
        task={task}
        plan={plan}
        planId={planId}
        panes={panes}
        onOpenPane={onOpenPane}
        onSection={onSection}
        renderTaskAttachmentThumbnails={renderTaskAttachmentThumbnails}
      />
    ) : null;
  }
  const displayPlan = resolvePlanModel(kind, plan, spec, specSections);
  if (!displayPlan) return null;
  const variant = kind === "spec" ? "spec" : "plan";
  const navPlanId = planId ?? displayPlan.id;
  const onSection = onSectionFrame ? (sectionId: string) => onSectionFrame({ planId: navPlanId, sectionId, variant }) : undefined;
  return <PlanDetail plan={displayPlan} variant={variant} onSection={onSection} />;
}
