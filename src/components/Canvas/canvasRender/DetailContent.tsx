import { RichMarkdown } from "@codeterm/chat-ui";
import { useState, type CSSProperties, type ReactNode } from "react";
import { ArrowUpRight, ChevronRight } from "lucide-react";
import { AgentIcon } from "../../AgentIcon";
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

const RELATED_TASKS_SHOWN = 1;

function tone(color: string): CSSProperties {
  return { "--ct-detail-tone": color } as CSSProperties;
}

function Block({ label, children, testId }: { label: string; children: ReactNode; testId?: string }) {
  return (
    <section data-testid={testId} className="ct-detail__block">
      <h3 className="ct-detail__heading">{label}</h3>
      {children}
    </section>
  );
}

function Markdown({ text }: { text: string }) {
  return (
    <div data-canvas-scroll-body="1" className="ct-detail__prose">
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

function TaskSectionLinks({ task, plan, planId, onSection }: {
  task: Task;
  plan?: Plan;
  planId?: string;
  onSection?: (a: SectionNavArgs) => void;
}) {
  const sections = linkedTaskSections(task, plan);
  if (sections.length === 0) return null;
  const navPlanId = planId ?? plan?.id;
  return (
    <Block label="Serves plan sections">
      <div className="ct-detail__links">
        {sections.map((section) => (
          <button
            key={section.id}
            type="button"
            className="ct-detail__link"
            data-testid="plan-detail-task-section-link"
            data-section-id={section.id}
            onClick={() => navPlanId && onSection?.({ planId: navPlanId, sectionId: section.id, variant: "plan" })}
          >
            {section.title}
            <ArrowUpRight size={12} aria-hidden="true" />
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

function LaneTab({ tab, onOpenTab }: { tab: TabIdentity; onOpenTab?: (tabId: string) => void }) {
  const canFocus = Boolean(tab.alive && onOpenTab);
  return (
    <span className="ct-detail__lane-tab">
      <button
        type="button"
        className="ct-detail__tab"
        data-testid="task-lane-tab"
        data-tab-id={tab.tabId}
        data-alive={tab.alive || undefined}
        disabled={!canFocus}
        title={tab.alive ? `Focus tab (${tab.tabId})` : `Tab ${tab.tabId} is closed`}
        onClick={() => canFocus && onOpenTab?.(tab.tabId)}
      >
        <span className="ct-detail__dot" aria-hidden="true" />
        <span data-testid="task-lane-tab-title">{tab.title ?? tab.tabId}</span>
      </button>
      {tab.title && <span data-testid="task-lane-tab-id" title="Tab id" className="ct-detail__mono ct-detail__faint">{tab.tabId}</span>}
      {!tab.alive && <span data-testid="task-lane-tab-dead" className="ct-detail__faint">closed</span>}
    </span>
  );
}

function AgentCard({ task, tabs, persona, agentTab, onOpenTab }: {
  task: Task;
  tabs: Record<string, Tab>;
  persona: string | null;
  agentTab: TabIdentity | null;
  onOpenTab?: (tabId: string) => void;
}) {
  const name = persona ?? agentTab?.title ?? (agentTab && !agentTab.alive ? `${agentTab.tabId} (closed)` : null);
  const lane = task.lane;
  const laneTab = resolveTab(lane?.tab_id, tabs);
  const rows = lane ? laneRows(lane) : [];
  const showLane = Boolean(lane && (laneTab || rows.length > 0));
  if (!name && !agentTab?.provider && !agentTab?.model && !showLane) return null;
  const tabId = agentTab?.tabId;
  const canOpen = Boolean(onOpenTab && tabId);
  const identity = (
    <>
      <span className="ct-detail__avatar" data-alive={agentTab?.alive || undefined}>
        <AgentIcon providerId={agentTab?.provider} />
      </span>
      <span className="ct-detail__agent-text">
        {persona
          ? <span className="ct-detail__persona" data-testid="task-personality" title="Executing persona">{persona}</span>
          : name && <span className="ct-detail__persona">{name}</span>}
        <span className="ct-detail__agent-sub">
          {name && <span className="sr-only" data-testid="task-agent-name">{name}</span>}
          {agentTab?.provider && <span data-testid="task-agent-provider">{agentTab.provider}</span>}
          {agentTab?.model && <span className="ct-detail__mono" data-testid="task-agent-model">{agentTab.model}</span>}
          {agentTab && !agentTab.alive && <span>tab closed</span>}
        </span>
      </span>
    </>
  );
  return (
    <div className="ct-card ct-card--quiet ct-detail__agent">
      {(name || agentTab?.provider || agentTab?.model) && (canOpen ? (
        <button type="button" className="ct-detail__agent-head" data-testid="plan-detail-open-tab" onClick={() => tabId && onOpenTab?.(tabId)}>
          <span className="contents" data-testid="task-agent-identity">{identity}</span>
          <span className="ct-detail__open">Open chat<ChevronRight size={13} aria-hidden="true" /></span>
        </button>
      ) : (
        <div className="ct-detail__agent-head" data-testid="task-agent-identity">{identity}</div>
      ))}
      {showLane && (
        <dl className="ct-detail__lane" data-testid="task-lane-panel">
          {laneTab && (
            <div><dt>tab</dt><dd><LaneTab tab={laneTab} onOpenTab={onOpenTab} /></dd></div>
          )}
          {rows.map((row) => (
            <div key={row.label}><dt>{row.label}</dt><dd className="ct-detail__mono" data-testid={`task-lane-${row.label}`}>{row.value}</dd></div>
          ))}
        </dl>
      )}
    </div>
  );
}

function TaskDetail({ api, task, plan, planId, tabs = {}, onOpenTab, onSection, renderTaskAttachmentThumbnails }: {
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
  const agentTab = resolveTab(task.assigned_tab_id, tabs);
  const persona = task.personality?.trim() || agentTab?.title || null;
  const attachmentThumbnails = renderTaskAttachmentThumbnails?.(task);
  const statusLabel = canonicalTaskStatusLabel(task, plan);
  return (
    <div className="ct-detail" data-testid="canvas-detail-content">
      <div className="ct-detail__meta">
        <span className="ct-detail__status" data-testid="status-chip" style={tone(TASK_STATUS_COLORS[task.status])}>
          <span className="ct-detail__dot" aria-hidden="true" />{statusLabel}
        </span>
        {task.priority && <span className="ct-detail__fact">{task.priority.toUpperCase()}</span>}
        {task.type && <span className="ct-detail__fact">{task.type}</span>}
      </div>
      {(task.executor_role?.trim() || task.role_guidance?.trim()) && (
        <section className="ct-detail__role" data-testid="task-role-guidance">
          <h3 className="ct-detail__heading">
            Role{task.executor_role?.trim() && <span className="ct-detail__role-name" data-testid="task-executor-role">{task.executor_role}</span>}
          </h3>
          {task.role_guidance?.trim() && <Markdown text={task.role_guidance} />}
        </section>
      )}
      <AgentCard task={task} tabs={tabs} persona={persona} agentTab={agentTab} onOpenTab={onOpenTab} />
      <Block label="Description" testId="canvas-detail-description">
        {body ? <Markdown text={body} /> : <span className="ct-detail__empty" data-testid="canvas-detail-empty-body">No description</span>}
      </Block>
      {task.acceptance_criteria?.length ? (
        <Block label="Acceptance criteria">
          <ul className="ct-detail__checks">
            {task.acceptance_criteria.map((c, i) => <li key={i}>{c}</li>)}
          </ul>
        </Block>
      ) : null}
      {task.test_plan?.trim() && <Block label="Test plan"><Markdown text={task.test_plan} /></Block>}
      {task.correctness?.trim() && <Block label="Correctness"><Markdown text={task.correctness} /></Block>}
      <AttachmentGallery api={api} planId={planId} attachments={task.attachments} />
      {attachmentThumbnails && (
        <Block label="Attachments">
          <div data-testid="plan-detail-attachment-thumbnails">{attachmentThumbnails}</div>
        </Block>
      )}
      <TaskSectionLinks task={task} plan={plan} planId={planId} onSection={onSection} />
      {task.external_refs?.length ? (
        <Block label="External references" testId="task-external-references">
          <div className="ct-detail__list">
            {task.external_refs.map((reference) => (
              <a
                key={`${reference.kind}:${reference.repository}#${reference.number}`}
                href={reference.canonical_url}
                target="_blank"
                rel="noopener noreferrer"
                className="ct-detail__href"
              >
                {reference.repository}#{reference.number}
                {reference.relationship ? ` · ${reference.relationship.replace("_", " ")}` : ""}
              </a>
            ))}
          </div>
        </Block>
      ) : null}
      {task.links?.length ? (
        <Block label="Links">
          <div className="ct-detail__list">
            {task.links.map((l, i) => <span key={i} className="ct-detail__href">{l}</span>)}
          </div>
        </Block>
      ) : null}
      <TaskActivityTimeline task={task} plan={plan} planId={planId} api={api} tabs={tabs} />
    </div>
  );
}

function sectionTaskGlyph(linked: Task[]): "done" | "progress" | "uncovered" {
  if (linked.length === 0) return "uncovered";
  if (linked.every((t) => t.status === "done" || t.status === "cancelled")) return "done";
  return "progress";
}

const GLYPH_TITLE = { done: "All tasks done", progress: "Tasks in progress", uncovered: "No tasks yet" } as const;

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
  return new Map(flattenSections(sections).map((section) => [section.id, section.title]));
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
  const [expanded, setExpanded] = useState(false);
  if (!ctx) return null;
  const linkedTasks = relationTasksForSection(section.id, ctx);
  const specTitles = sectionTitleById(ctx.specSections);
  const planTitles = sectionTitleById(ctx.planSections);
  const specLinks = ctx.specLinks ?? [];
  const sectionLinks = ctx.variant === "plan"
    ? specLinks.filter((link) => link.plan_section_id === section.id).map((link) => ({
      id: link.spec_section_id,
      label: specTitles.get(link.spec_section_id) ?? "linked section",
      variant: "spec" as const,
      testId: "canvas-detail-spec-section-chip",
    }))
    : specLinks.filter((link) => link.spec_section_id === section.id).map((link) => ({
      id: link.plan_section_id,
      label: planTitles.get(link.plan_section_id) ?? "linked section",
      variant: "plan" as const,
      testId: "canvas-detail-plan-section-chip",
    }));
  if (sectionLinks.length === 0 && linkedTasks.length === 0) return null;
  const shown = expanded ? linkedTasks : linkedTasks.slice(0, RELATED_TASKS_SHOWN);
  const hidden = linkedTasks.length - shown.length;
  return (
    <div data-testid="canvas-detail-section-relations" className="ct-detail__relations">
      {sectionLinks.map((link) => (
        <button
          key={`${link.variant}:${link.id}`}
          type="button"
          className="ct-detail__link"
          data-testid={link.testId}
          disabled={!ctx.onSection}
          onClick={() => ctx.onSection?.(link.id, link.variant)}
        >
          {link.variant === "spec" ? "Spec: " : "Plan: "}{link.label}
        </button>
      ))}
      {shown.map((task) => (
        <button
          key={task.id}
          type="button"
          className="ct-detail__task"
          style={tone(TASK_STATUS_COLORS[task.status])}
          data-testid="canvas-detail-task-chip"
          disabled={!ctx.onOpenTask}
          onClick={() => ctx.onOpenTask?.(task.id)}
        >
          {taskReferenceLabel(task)}
        </button>
      ))}
      {hidden > 0 && (
        <button type="button" className="ct-detail__more" data-testid="canvas-detail-more-tasks" onClick={() => setExpanded(true)}>
          +{hidden} more
        </button>
      )}
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
          <div key={s.id} data-testid="canvas-detail-section" data-section-id={s.id} data-depth={depth || undefined} className="ct-detail__section">
            <div className="ct-detail__section-head" style={tone(GLYPH_ACCENT[glyph])}>
              {ctx && <span className="ct-detail__dot" title={GLYPH_TITLE[glyph]} />}
              <button
                type="button"
                className="ct-detail__section-title"
                data-testid="canvas-detail-section-nav"
                title={taskCount > 0 ? `Go to ${taskCount} task${taskCount > 1 ? "s" : ""} in this section` : "No tasks in this section"}
                onClick={() => ctx?.onSection?.(s.id, ctx.variant)}
              >
                {s.title}
              </button>
              {taskCount > 0 && (
                <span className="ct-detail__count" data-testid="canvas-detail-section-task-count">
                  {taskCount} task{taskCount > 1 ? "s" : ""}
                </span>
              )}
            </div>
            <div className="ct-detail__section-body">
              {s.body?.trim() ? <Markdown text={s.body} /> : null}
              <SectionRelations section={s} ctx={ctx} />
              {s.tags?.length ? (
                <div className="ct-detail__tags">
                  {s.tags.map((t) => <span key={t}>{t.includes(":") ? t.slice(t.indexOf(":") + 1).replace(/-/g, " ") : t}</span>)}
                </div>
              ) : null}
              {s.subsections?.length ? <SectionTree sections={s.subsections} depth={depth + 1} ctx={ctx} /> : null}
            </div>
          </div>
        );
      })}
    </>
  );
}

function Progress({ done, total, testId, children }: { done: number; total: number; testId: string; children?: ReactNode }) {
  const pct = total ? Math.round((done / total) * 100) : 0;
  return (
    <div className="ct-detail__progress" data-testid={testId}>
      {children}
      <div className="ct-detail__bar"><span style={{ width: `${pct}%` }} /></div>
    </div>
  );
}

function SpecCoverageHeader({ plan }: { plan: Plan }) {
  const activeTasks = plan.tasks.filter((task) => task.status !== "cancelled");
  const total = activeTasks.length;
  if (total === 0) return null;
  const done = activeTasks.filter((task) => task.status === "done").length;
  const active = total - done;
  const pct = Math.round((done / total) * 100);
  return (
    <Progress done={done} total={total} testId="canvas-detail-spec-coverage">
      <div className="ct-detail__progress-line">
        <span className="ct-detail__progress-value" data-testid="canvas-detail-spec-total">{done}/{total}</span>
        <span>tasks done</span>
        {done > 0 && <span className="ct-detail__fact" data-testid="canvas-detail-spec-covered" style={tone(GLYPH_ACCENT.done)}><span className="ct-detail__dot" aria-hidden="true" />{done} done</span>}
        {active > 0 && <span className="ct-detail__fact" data-testid="canvas-detail-spec-partial" style={tone(GLYPH_ACCENT.progress)}><span className="ct-detail__dot" aria-hidden="true" />{active} active</span>}
        <span className="ct-detail__faint" data-testid="canvas-detail-spec-percent">{pct}% done</span>
      </div>
    </Progress>
  );
}

function PlanDetail({ plan, variant, planSections, specSections = [], tabs = {}, planId, api, onSection, onOpenTask }: {
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
  const ownerTab = plan.owner.tab_id ? tabs[plan.owner.tab_id] : undefined;
  const ownerLabel = ownerTab?.title?.trim() || plan.owner.title?.trim() || null;
  const goal = plan.goal?.vector?.trim();
  const metaBits = [
    plan.mode === "paused" ? "Parked" : plan.status === "active" ? "Active" : plan.status,
    ownerLabel ? `led by ${ownerLabel}` : null,
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
    <div className="ct-detail" data-testid="canvas-detail-content">
      {variant === "plan" && (
        <div className="ct-detail__block">
          {goal && goal !== plan.title.trim() && <p className="ct-detail__goal">{goal}</p>}
          {total > 0 && (
            <Progress done={done} total={total} testId="canvas-detail-progress">
              <div className="ct-detail__progress-line">
                <span>{done} / {total} tasks done</span>
                {metaBits.length > 0 && <span className="ct-detail__faint">{metaBits.join(" · ")}</span>}
              </div>
            </Progress>
          )}
          {total === 0 && metaBits.length > 0 && <p className="ct-detail__faint">{metaBits.join(" · ")}</p>}
        </div>
      )}
      {variant === "spec" && <SpecCoverageHeader plan={plan} />}
      {variant === "plan" && <PlanRetrospectiveBlock plan={plan} />}
      <Block label={variant === "spec" ? "Specification" : "Sections"}>
        <div className="ct-detail__sections">
          {sections.length
            ? <SectionTree sections={sections} ctx={relationCtx} />
            : <span className="ct-detail__empty">No sections.</span>}
        </div>
      </Block>
      {variant === "plan" && <PlanRecentActivityRail plan={plan} tabs={tabs} onOpenTask={onOpenTask} />}
      {variant === "plan" && planId && api && <PlanSkillsSection planId={planId} plan={plan} api={api} compact />}
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
