import { RichMarkdown } from "@codeterm/chat-ui";
import type { CSSProperties, ReactNode } from "react";
import type { ApiConfig, Tab } from "../../../lib/types";
import { TASK_STATUS_COLORS } from "../settings";
import type { Plan, Section, Task, TaskLaneMetadata } from "../types";
import type { SectionNavArgs } from "../nodeRegistry";
import type { PlanDetailKind } from "../planView/PlanDetailOverlay";
import { canonicalTaskStatusLabel } from "../planView/taskStatusLabel";
import { AttachmentGallery } from "../planView/attachments";
import { PlanRecentActivityRail } from "../planView/PlanRecentActivityRail.tsx";
import { PlanRetrospectiveBlock } from "../planView/PlanRetrospectiveBlock.tsx";
import { PlanSkillsSection } from "../planView/PlanSkillsSection.tsx";
import { TaskActivityTimeline } from "../planView/TaskActivityTimeline.tsx";
import { GLYPH_ACCENT } from "./cardContent";
import { taskReferenceLabel } from "../taskDisplayKey";


export interface DetailContentProps {
  kind: PlanDetailKind;
  planId?: string;
  api?: ApiConfig;
  task?: Task;
  plan?: Plan;
  spec?: Plan;
  specSections?: Section[];
  tabs?: Record<string, Tab>;
  onOpenTab?: (tabId: string) => void;
  onOpenTask?: (taskId: string) => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
  renderTaskAttachmentThumbnails?: (task: Task) => ReactNode;
}

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

function Chip({ label, value, color, testId }: { label: string; value: string; color?: string; testId?: string }) {
  return (
    <span data-testid={testId} style={{ display: "inline-flex", alignItems: "center", gap: 6, padding: "3px 9px", borderRadius: 999, border: "1px solid var(--ct-border-subtle)", fontSize: 12, background: "color-mix(in srgb, var(--ct-fg) 5%, transparent)" }}>
      <span style={{ ...labelStyle, fontSize: 10 }}>{label}</span>
      <span style={{ display: "inline-flex", alignItems: "center", gap: 5, fontWeight: 600 }}>
        {color && <span style={{ width: 7, height: 7, borderRadius: "50%", background: color }} />}
        {value}
      </span>
    </span>
  );
}

function RelationChip({ children, onClick, testId }: { children: ReactNode; onClick?: () => void; testId: string }) {
  const style: CSSProperties = {
    display: "inline-flex",
    alignItems: "center",
    minWidth: 0,
    maxWidth: 240,
    padding: "2px 8px",
    borderRadius: 999,
    border: "1px solid var(--ct-border-subtle)",
    background: "color-mix(in srgb, var(--ct-accent) 9%, transparent)",
    color: "var(--ct-fg)",
    fontSize: 11.5,
    fontWeight: 600,
    cursor: onClick ? "pointer" : "default",
  };
  if (!onClick) return <span data-testid={testId} style={style}>{children}</span>;
  return (
    <button type="button" data-testid={testId} onClick={onClick} style={style}>
      <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{children}</span>
    </button>
  );
}

function Block({ label, children, testId }: { label: string; children: ReactNode; testId?: string }) {
  return (
    <div data-testid={testId} style={{ display: "flex", flexDirection: "column", gap: 6 }}>
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

interface TabIdentity {
  tabId: string;
  title: string | null;
  provider: string | null;
  model: string | null;
  alive: boolean;
}

function resolveTab(tabId: string | null | undefined, tabs: Record<string, Tab>): TabIdentity | null {
  const id = tabId?.trim();
  if (!id) return null;
  const tab = tabs[id];
  return {
    tabId: id,
    title: tab?.title?.trim() || tab?.agent_session_name?.trim() || null,
    provider: tab?.agent_type?.trim() || null,
    model: tab?.agent_session_model?.trim() || null,
    alive: Boolean(tab),
  };
}

function laneRows(lane: TaskLaneMetadata): { label: string; value: string }[] {
  const rows: { label: string; value: string }[] = [];
  const branch = lane.branch?.trim();
  const worktree = lane.worktree_path?.trim();
  const workspace = lane.workspace_name?.trim() || lane.workspace_id?.trim();
  if (branch) rows.push({ label: "branch", value: branch });
  if (worktree) rows.push({ label: "worktree", value: worktree });
  if (workspace) rows.push({ label: "workspace", value: workspace });
  return rows;
}

function LanePanel({ lane, tabs, onOpenTab }: { lane?: TaskLaneMetadata; tabs: Record<string, Tab>; onOpenTab?: (tabId: string) => void }) {
  if (!lane) return null;
  const tabId = lane.tab_id?.trim() || null;
  const rows = laneRows(lane);
  if (!tabId && rows.length === 0) return null;
  const tab = resolveTab(tabId, tabs);
  const canFocus = Boolean(tab?.alive && onOpenTab && tabId);
  const monoValue: CSSProperties = { fontFamily: "var(--ct-font-mono)", fontSize: 12, wordBreak: "break-all", color: "var(--ct-fg)" };
  const tabLabel = tab?.title ?? tab?.tabId ?? "";
  return (
    <Block label="Lane" testId="task-lane-panel">
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {tab && tabId && (
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <span style={{ ...labelStyle, fontSize: 10 }}>tab</span>
            <button
              type="button"
              data-testid="task-lane-tab"
              data-tab-id={tab.tabId}
              disabled={!canFocus}
              title={tab.alive ? `Focus tab (${tab.tabId})` : `Tab ${tab.tabId} is closed`}
              onClick={() => canFocus && onOpenTab!(tabId)}
              style={{ display: "inline-flex", alignItems: "center", gap: 6, padding: "3px 9px", borderRadius: 999, border: "1px solid var(--ct-border-subtle)", background: canFocus ? "color-mix(in srgb, var(--ct-accent) 9%, transparent)" : "color-mix(in srgb, var(--ct-fg) 5%, transparent)", color: canFocus ? "var(--ct-accent)" : "var(--ct-fg)", fontSize: 12, fontWeight: 600, cursor: canFocus ? "pointer" : "default" }}
            >
              <span style={{ width: 7, height: 7, borderRadius: "50%", background: tab.alive ? GLYPH_ACCENT.progress : "color-mix(in srgb, var(--ct-fg) 35%, transparent)" }} />
              <span data-testid="task-lane-tab-title">{tabLabel}</span>
            </button>
            {tab.title && <span data-testid="task-lane-tab-id" title="Tab id" style={{ ...monoValue, fontSize: 11, color: "color-mix(in srgb, var(--ct-fg) 45%, transparent)" }}>{tab.tabId}</span>}
            {!tab.alive && <span data-testid="task-lane-tab-dead" style={{ fontSize: 11, fontStyle: "italic", color: "color-mix(in srgb, var(--ct-fg) 50%, transparent)" }}>closed</span>}
          </div>
        )}
        {rows.map((row) => (
          <div key={row.label} style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
            <span style={{ ...labelStyle, fontSize: 10, minWidth: 68 }}>{row.label}</span>
            <span data-testid={`task-lane-${row.label}`} style={monoValue}>{row.value}</span>
          </div>
        ))}
      </div>
    </Block>
  );
}

function PersonaChip({ persona }: { persona: string }) {
  return (
    <span
      data-testid="task-personality"
      title="Executing persona"
      style={{ display: "inline-flex", alignItems: "center", gap: 6, padding: "3px 11px", borderRadius: 999, border: "1px solid color-mix(in srgb, var(--ct-accent) 40%, transparent)", background: "color-mix(in srgb, var(--ct-accent) 14%, transparent)", color: "var(--ct-accent)", fontSize: 12.5, fontWeight: 700 }}
    >
      {persona}
    </span>
  );
}

function AgentIdentity({ tab, persona }: { tab: TabIdentity | null; persona: string | null }) {
  const name = persona ?? tab?.title ?? (tab && !tab.alive ? `${tab.tabId} (closed)` : null);
  if (!name && !tab?.provider && !tab?.model) return null;
  return (
    <Block label="Agent" testId="task-agent-identity">
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center" }}>
        {name && <Chip label="tab" value={name} testId="task-agent-name" />}
        {tab?.provider && <Chip label="provider" value={tab.provider} testId="task-agent-provider" />}
        {tab?.model && <Chip label="model" value={tab.model} testId="task-agent-model" />}
      </div>
    </Block>
  );
}

function TaskDetail({
  api,
  task,
  plan,
  planId,
  tabs = {},
  onOpenTab,
  onSection,
  renderTaskAttachmentThumbnails,
}: {
  api?: ApiConfig;
  task: Task;
  plan?: Plan;
  planId?: string;
  tabs?: Record<string, Tab>;
  onOpenTab?: (tabId: string) => void;
  onSection?: (a: SectionNavArgs) => void;
  renderTaskAttachmentThumbnails?: (task: Task) => ReactNode;
}) {
  const body = task.body?.trim();
  const tabId = task.assigned_tab_id?.trim() || null;
  const agentTab = resolveTab(tabId, tabs);
  const persona = task.personality?.trim() || agentTab?.title || null;
  const attachmentThumbnails = renderTaskAttachmentThumbnails?.(task);
  const statusLabel = canonicalTaskStatusLabel(task);
  return (
    <div style={wrap} data-testid="canvas-detail-content">
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center" }}>
        <Chip label="status" value={statusLabel} color={TASK_STATUS_COLORS[task.status]} testId="status-chip" />
        {persona && <PersonaChip persona={persona} />}
        {task.priority && <Chip label="priority" value={task.priority.toUpperCase()} />}
        {task.type && <Chip label="type" value={task.type} />}
        {task.executor_role?.trim() && <Chip label="role" value={task.executor_role} testId="task-executor-role" />}
        {onOpenTab && tabId && (
          <button
            type="button"
            data-testid="plan-detail-open-tab"
            onClick={() => onOpenTab(tabId)}
            style={{ marginLeft: "auto", padding: "4px 10px", borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-accent)", fontSize: 12, fontWeight: 600, cursor: "pointer" }}
          >
            Open tab
          </button>
        )}
      </div>
      <AgentIdentity tab={agentTab} persona={persona} />
      <LanePanel lane={task.lane} tabs={tabs} onOpenTab={onOpenTab} />
      <TaskSectionLinks task={task} plan={plan} planId={planId} onSection={onSection} />
      {task.external_refs?.length ? (
        <Block label="External references" testId="task-external-references">
          <div style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
            {task.external_refs.map((reference) => (
              <a
                key={`${reference.kind}:${reference.repository}#${reference.number}`}
                href={reference.canonical_url}
                target="_blank"
                rel="noopener noreferrer"
                style={{ color: "var(--ct-accent)", wordBreak: "break-all" }}
              >
                {reference.repository}#{reference.number}
                {reference.relationship ? ` · ${reference.relationship.replace("_", " ")}` : ""}
              </a>
            ))}
          </div>
        </Block>
      ) : null}
      {attachmentThumbnails && (
        <Block label="Attachments">
          <div data-testid="plan-detail-attachment-thumbnails">{attachmentThumbnails}</div>
        </Block>
      )}
      {task.role_guidance?.trim() && (
        <Block label="Role guidance" testId="task-role-guidance">
          <Markdown text={task.role_guidance} />
        </Block>
      )}
      <Block label="Description" testId="canvas-detail-description">
        {body ? <Markdown text={body} /> : <span style={muted} data-testid="canvas-detail-empty-body">No description</span>}
      </Block>
      <AttachmentGallery api={api} planId={planId} attachments={task.attachments} />
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
      <TaskActivityTimeline task={task} plan={plan} planId={planId} api={api} tabs={tabs} />
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

function sectionTaskGlyph(linked: Task[]): "done" | "progress" | "uncovered" {
  if (linked.length === 0) return "uncovered";
  if (linked.every((t) => t.status === "done" || t.status === "cancelled")) return "done";
  return "progress";
}

function flattenSectionTree(sections: Section[]): Section[] {
  return sections.flatMap((section) => [section, ...flattenSectionTree(section.subsections ?? [])]);
}

interface RelationContext {
  variant: "plan" | "spec";
  planId: string;
  tasks: Task[];
  planSections: Section[];
  specSections: Section[];
  specLinks: Plan["spec_links"];
  onSection?: (sectionId: string, variant: "plan" | "spec") => void;
  onOpenTask?: (taskId: string) => void;
}

function sectionTitleById(sections: Section[]): Map<string, string> {
  return new Map(flattenSectionTree(sections).map((section) => [section.id, section.title]));
}

function relationTasksForSection(sectionId: string, ctx?: RelationContext): Task[] {
  if (!ctx) return [];
  if (ctx.variant === "plan") return ctx.tasks.filter((task) => (task.section_ids ?? []).includes(sectionId));
  const planSectionIds = new Set((ctx.specLinks ?? [])
    .filter((link) => link.spec_section_id === sectionId)
    .map((link) => link.plan_section_id));
  return ctx.tasks.filter((task) => (task.section_ids ?? []).some((id) => planSectionIds.has(id)));
}

function SectionRelations({ section, ctx }: { section: Section; ctx?: RelationContext }) {
  if (!ctx) return null;
  const linkedTasks = relationTasksForSection(section.id, ctx);
  const specTitles = sectionTitleById(ctx.specSections);
  const planTitles = sectionTitleById(ctx.planSections);
  const specLinks = ctx.specLinks ?? [];
  const sectionLinks = ctx.variant === "plan"
    ? specLinks.filter((link) => link.plan_section_id === section.id).map((link) => ({
      id: link.spec_section_id,
      label: specTitles.get(link.spec_section_id) ?? link.spec_section_id,
      variant: "spec" as const,
      testId: "canvas-detail-spec-section-chip",
    }))
    : specLinks.filter((link) => link.spec_section_id === section.id).map((link) => ({
      id: link.plan_section_id,
      label: planTitles.get(link.plan_section_id) ?? link.plan_section_id,
      variant: "plan" as const,
      testId: "canvas-detail-plan-section-chip",
    }));
  if (sectionLinks.length === 0 && linkedTasks.length === 0) return null;
  return (
    <div data-testid="canvas-detail-section-relations" style={{ display: "flex", flexWrap: "wrap", gap: 6, marginLeft: ctx.tasks.length ? 16 : 0 }}>
      {sectionLinks.map((link) => (
        <RelationChip
          key={`${link.variant}:${link.id}`}
          testId={link.testId}
          onClick={ctx.onSection ? () => ctx.onSection?.(link.id, link.variant) : undefined}
        >
          {link.variant === "spec" ? "Spec: " : "Plan: "}{link.label}
        </RelationChip>
      ))}
      {linkedTasks.map((task) => (
        <RelationChip
          key={task.id}
          testId="canvas-detail-task-chip"
          onClick={ctx.onOpenTask ? () => ctx.onOpenTask?.(task.id) : undefined}
        >
          {taskReferenceLabel(task)}
        </RelationChip>
      ))}
    </div>
  );
}

function SectionTree({ sections, depth = 0, ctx }: { sections: Section[]; depth?: number; ctx?: RelationContext }) {
  return (
    <>
      {sections.map((s) => {
        const linkedTasks = relationTasksForSection(s.id, ctx);
        const glyph = ctx ? sectionTaskGlyph(linkedTasks) : "uncovered";
        const taskCount = linkedTasks.length;
        return (
          <div key={s.id} data-testid="canvas-detail-section" data-section-id={s.id} style={{ display: "flex", flexDirection: "column", gap: 6, marginLeft: depth * 16, paddingTop: depth ? 4 : 0 }}>
            <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
              {ctx && <SectionCoverageGlyph glyph={glyph} />}
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ display: "flex", alignItems: "baseline", gap: 8, flexWrap: "wrap" }}>
                  <button
                    type="button"
                    data-testid="canvas-detail-section-nav"
                    title={taskCount > 0 ? `Go to ${taskCount} task${taskCount > 1 ? "s" : ""} in this section` : "No tasks in this section"}
                    onClick={() => ctx?.onSection?.(s.id, ctx.variant)}
                    style={{ ...sectionTitle, fontSize: depth ? 13.5 : 15, display: "inline", padding: 0, border: "none", background: "transparent", color: "var(--ct-fg)", cursor: ctx?.onSection ? "pointer" : "default", textAlign: "left" }}
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
            <SectionRelations section={s} ctx={ctx} />
            {s.tags?.length ? (
              <div style={{ display: "flex", flexWrap: "wrap", gap: 5, marginLeft: ctx ? 16 : 0 }}>
                {s.tags.map((t) => <span key={t} style={{ fontSize: 11, padding: "1px 7px", borderRadius: 999, background: "color-mix(in srgb, var(--ct-fg) 7%, transparent)" }}>{t.includes(":") ? t.slice(t.indexOf(":") + 1).replace(/-/g, " ") : t}</span>)}
              </div>
            ) : null}
            {s.body?.trim() ? <div style={{ marginLeft: ctx ? 16 : 0 }}><Markdown text={s.body} /></div> : null}
            {s.subsections?.length ? <SectionTree sections={s.subsections} depth={depth + 1} ctx={ctx} /> : null}
          </div>
        );
      })}
    </>
  );
}

function SpecCoverageHeader({ plan }: { plan: Plan }) {
  const activeTasks = plan.tasks.filter((task) => task.status !== "cancelled");
  const total = activeTasks.length;
  if (total === 0) return null;
  const done = activeTasks.filter((task) => task.status === "done").length;
  const active = activeTasks.filter((task) => task.status !== "done").length;
  const pct = Math.round((done / total) * 100);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }} data-testid="canvas-detail-spec-coverage">
      <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
        <span data-testid="canvas-detail-spec-total" style={{ fontSize: 28, fontWeight: 800, lineHeight: 1 }}>{done}/{total}</span>
        <span style={{ fontSize: 13, color: "color-mix(in srgb, var(--ct-fg) 60%, transparent)" }}>
          tasks done
        </span>
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
        {done > 0 && (
          <span data-testid="canvas-detail-spec-covered" style={{ display: "inline-flex", alignItems: "center", gap: 5, fontSize: 12, fontWeight: 600 }}>
            <span style={{ width: 8, height: 8, borderRadius: "50%", background: GLYPH_ACCENT.done }} />
            {done} done
          </span>
        )}
        {active > 0 && (
          <span data-testid="canvas-detail-spec-partial" style={{ display: "inline-flex", alignItems: "center", gap: 5, fontSize: 12, fontWeight: 600 }}>
            <span style={{ width: 8, height: 8, borderRadius: "50%", background: GLYPH_ACCENT.progress }} />
            {active} active
          </span>
        )}
        <span data-testid="canvas-detail-spec-percent" style={{ fontSize: 12, color: "color-mix(in srgb, var(--ct-fg) 45%, transparent)" }}>{pct}% done</span>
      </div>
      <div style={{ height: 5, borderRadius: 999, background: "color-mix(in srgb, var(--ct-fg) 10%, transparent)", overflow: "hidden" }}>
        <div style={{ width: `${pct}%`, height: "100%", background: GLYPH_ACCENT.done }} />
      </div>
    </div>
  );
}

function PlanDetail({
  plan,
  variant,
  planSections,
  specSections = [],
  tabs = {},
  planId,
  api,
  onSection,
  onOpenTask,
}: {
  plan: Plan;
  variant: "plan" | "spec";
  planSections: Section[];
  specSections?: Section[];
  tabs?: Record<string, Tab>;
  planId?: string;
  api?: ApiConfig;
  onSection?: (sectionId: string, variant: "plan" | "spec") => void;
  onOpenTask?: (taskId: string) => void;
}) {
  const sections = plan.sections;
  const activeTasks = plan.tasks.filter((task) => task.status !== "cancelled");
  const total = activeTasks.length;
  const done = activeTasks.filter((task) => task.status === "done").length;
  const pct = total ? Math.round((done / total) * 100) : 0;
  const metaBits = [
    plan.mode === "paused" ? "Parked" : plan.status === "active" ? "Active" : plan.status,
    plan.owner.title || plan.owner.tab_id ? `owner ${plan.owner.title ?? plan.owner.tab_id}` : null,
    plan.primary_spec_id ? "linked spec" : null,
    plan.loops?.length ? `${plan.loops.length} loop${plan.loops.length > 1 ? "s" : ""}` : null,
  ].filter(Boolean) as string[];
  const relationCtx: RelationContext = {
    variant,
    planId: plan.id,
    tasks: plan.tasks,
    planSections,
    specSections,
    specLinks: plan.spec_links,
    onSection,
    onOpenTask,
  };

  return (
    <div style={wrap} data-testid="canvas-detail-content">
      {variant === "plan" && <PlanRetrospectiveBlock plan={plan} />}
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
      {variant === "plan" && <PlanRecentActivityRail plan={plan} tabs={tabs} onOpenTask={onOpenTask} />}
      {variant === "plan" && planId && api && (
        <PlanSkillsSection planId={planId} plan={plan} api={api} />
      )}
      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <div style={labelStyle}>{variant === "spec" ? "Specification" : "Sections"}</div>
        {sections.length
          ? <SectionTree sections={sections} ctx={relationCtx} />
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

export function DetailContent({ kind, planId, api, task, plan, spec, specSections, tabs, onOpenTab, onOpenTask, onSectionClick, onSectionFrame, renderTaskAttachmentThumbnails }: DetailContentProps) {
  if (kind === "task") {
    const onSection = onSectionFrame ?? onSectionClick;
    return task ? (
      <TaskDetail
        api={api}
        task={task}
        plan={plan}
        planId={planId}
        tabs={tabs}
        onOpenTab={onOpenTab}
        onSection={onSection}
        renderTaskAttachmentThumbnails={renderTaskAttachmentThumbnails}
      />
    ) : null;
  }
  const displayPlan = resolvePlanModel(kind, plan, spec, specSections);
  if (!displayPlan) return null;
  const variant = kind === "spec" ? "spec" : "plan";
  const navPlanId = planId ?? displayPlan.id;
  const onSectionNav = onSectionFrame ?? onSectionClick;
  const onSection = onSectionNav
    ? (sectionId: string, targetVariant: "plan" | "spec") => onSectionNav({ planId: navPlanId, sectionId, variant: targetVariant })
    : undefined;
  return (
    <PlanDetail
      plan={displayPlan}
      variant={variant}
      planSections={kind === "spec" ? (spec?.sections ?? []) : displayPlan.sections}
      specSections={specSections ?? (kind === "spec" ? displayPlan.sections : [])}
      tabs={tabs}
      planId={navPlanId}
      api={api}
      onSection={onSection}
      onOpenTask={onOpenTask}
    />
  );
}
