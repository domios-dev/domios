import { RichMarkdown } from "@codeterm/chat-ui";
import type { CSSProperties } from "react";
import { TASK_STATUS_COLORS } from "../settings";
import type { Plan, Section, Task } from "../types";
import type { LaidOutNode } from "../layout";
import type { SectionNavArgs } from "../nodeRegistry";

// The READ surface body for the detail overlay. Plain (no section tint) — structure
// + rendered markdown only. Task: description + structured fields. Plan/spec: progress
// (plan) + the SECTIONS as the hero, each body rendered as markdown; meta is a compact
// secondary strip, never the headline.

export interface DetailContentProps {
  node: LaidOutNode;
  model: unknown;
  onOpenPane?: (paneId: string) => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
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

function TaskDetail({ task }: { task: Task }) {
  const body = task.body?.trim();
  return (
    <div style={wrap} data-testid="canvas-detail-content">
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
        <Chip label="status" value={task.status_label?.trim() || task.status.replace(/_/g, " ")} color={TASK_STATUS_COLORS[task.status]} />
        {task.priority && <Chip label="priority" value={task.priority.toUpperCase()} />}
        {task.type && <Chip label="type" value={task.type} />}
        {(task.assigned_pane_id || task.lane?.pane_id) && <Chip label="assignee" value={(task.assigned_pane_id || task.lane?.pane_id)!} />}
      </div>
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

function SectionTree({ sections, depth = 0, onSection }: { sections: Section[]; depth?: number; onSection?: (sectionId: string) => void }) {
  return (
    <>
      {sections.map((s) => (
        <div key={s.id} data-testid="canvas-detail-section" data-section-id={s.id} style={{ display: "flex", flexDirection: "column", gap: 6, marginLeft: depth * 16, paddingTop: depth ? 4 : 0 }}>
          <button
            type="button"
            data-testid="canvas-detail-section-nav"
            title="Go to this section's tasks"
            onClick={() => onSection?.(s.id)}
            style={{ ...sectionTitle, fontSize: depth ? 13.5 : 15, display: "flex", alignItems: "center", gap: 7, padding: 0, border: "none", background: "transparent", color: "var(--ct-fg)", cursor: onSection ? "pointer" : "default", textAlign: "left" }}
          >
            <span>{s.title}</span>
            {onSection && <span style={{ fontSize: 11, fontWeight: 600, color: "var(--ct-accent)", opacity: 0.85 }}>→ tasks</span>}
          </button>
          {s.tags?.length ? (
            <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
              {s.tags.map((t) => <span key={t} style={{ fontSize: 11, padding: "1px 7px", borderRadius: 999, background: "color-mix(in srgb, var(--ct-fg) 7%, transparent)" }}>{t.includes(":") ? t.slice(t.indexOf(":") + 1).replace(/-/g, " ") : t}</span>)}
            </div>
          ) : null}
          {s.body?.trim() ? <Markdown text={s.body} /> : null}
          {s.subsections?.length ? <SectionTree sections={s.subsections} depth={depth + 1} onSection={onSection} /> : null}
        </div>
      ))}
    </>
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
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }} data-testid="canvas-detail-progress">
              <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
                <span style={{ fontSize: 28, fontWeight: 800, lineHeight: 1 }}>{pct}%</span>
                <span style={{ fontSize: 13, color: "color-mix(in srgb, var(--ct-fg) 60%, transparent)" }}>{done} / {total} tasks done</span>
              </div>
              <div style={{ height: 7, borderRadius: 999, background: "color-mix(in srgb, var(--ct-fg) 10%, transparent)", overflow: "hidden" }}>
                <div style={{ width: `${pct}%`, height: "100%", background: "var(--ct-accent)" }} />
              </div>
            </div>
          )}
          {plan.goal?.vector && <div style={{ fontSize: 13.5, lineHeight: 1.5, color: "var(--ct-fg)" }}><span style={{ ...labelStyle, marginRight: 8 }}>Goal</span>{plan.goal.vector}</div>}
          {metaBits.length > 0 && <div style={{ fontSize: 12, color: "color-mix(in srgb, var(--ct-fg) 55%, transparent)" }}>{metaBits.join("  ·  ")}</div>}
        </div>
      )}
      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <div style={labelStyle}>{variant === "spec" ? "Specification" : "Sections"}</div>
        {sections.length ? <SectionTree sections={sections} onSection={onSection} /> : <span style={muted}>No sections.</span>}
      </div>
    </div>
  );
}

export function DetailContent({ node, model, onSectionFrame }: DetailContentProps) {
  if (model == null) return null;
  if (node.kind === "task") {
    const m = model as { task: Task };
    return m.task ? <TaskDetail task={m.task} /> : null;
  }
  // Click a section → fly/focus its tasks on the canvas (the overlay closes first).
  const variant = node.kind === "spec" ? "spec" : "plan";
  const onSection = onSectionFrame ? (sectionId: string) => onSectionFrame({ planId: node.planId, sectionId, variant }) : undefined;
  if (node.kind === "spec" || node.kind === "plan") return <PlanDetail plan={model as Plan} variant={variant} onSection={onSection} />;
  return null;
}
