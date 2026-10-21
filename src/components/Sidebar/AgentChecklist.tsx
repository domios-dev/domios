import { useCallback, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { Circle, CircleAlert, CircleCheck, CircleDot, CircleMinus, ListChecks } from "lucide-react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { usePlanTargetNavigation } from "../Canvas/teamViewNavigation";
import type { Plan } from "../Canvas/types";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import {
  boundedChecklist,
  checklistStateLabel,
  eventNeedsReread,
  isTabTodoChanged,
  TAB_TODO_CHANGED_EVENT,
  type AgentChecklist,
  type AgentChecklistItem,
  type AgentChecklistState,
} from "./agentChecklistModel";

export interface ChecklistToggle {
  expanded: boolean;
  controlsId: string;
  onToggle: () => void;
}

type ChecklistView =
  | { status: "loading" }
  | { status: "error" }
  | { status: "ready"; checklist: AgentChecklist | null };

/** Mounted only while the detail is open, so the subscription never outlives the disclosure. */
function useAgentChecklist(tabId: string): ChecklistView {
  const [view, setView] = useState<ChecklistView>({ status: "loading" });
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    let shown: AgentChecklist | null = null;
    let issued = 0;
    let applied = 0;
    const reread = () => {
      const request = ++issued;
      invoke<unknown>("tab_todo_list", { tabId }).then((raw) => {
        if (disposed || request < applied) return;
        applied = request;
        shown = boundedChecklist(raw);
        setView({ status: "ready", checklist: shown });
      }, () => {
        if (!disposed && request === issued) setView({ status: "error" });
      });
    };
    listen<unknown>(TAB_TODO_CHANGED_EVENT, ({ payload }) => {
      if (!disposed && isTabTodoChanged(payload) && eventNeedsReread(tabId, shown, payload)) reread();
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
      // Read after subscribing so a change landing in between still triggers a reread.
      if (!disposed) reread();
    }, () => {
      if (!disposed) reread();
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [tabId]);
  return view;
}

function StateGlyph({ state }: { state: AgentChecklistState }) {
  const Icon = state === "done" ? CircleCheck
    : state === "in_progress" ? CircleDot
    : state === "blocked" ? CircleAlert
    : state === "skipped" ? CircleMinus
    : Circle;
  return <Icon size={10} strokeWidth={1.75} aria-hidden="true" className="sidebar-agent-checklist__glyph" />;
}

function ChecklistTaskLink({ target, label, onOpen }: { target: { planId: string; taskId: string }; label: string; onOpen: () => void }) {
  const navigate = usePlanTargetNavigation();
  return <button type="button" className="text-[var(--ct-accent)] text-left underline" onClick={() => { navigate(target); onOpen(); }}>{label}</button>;
}

function SidebarAgentChecklist({ item, plans, tabId }: { item: AgentChecklistItem; plans: readonly Plan[]; tabId: string }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLLIElement>(null);
  const detailId = useId();
  const linked = item.link?.kind === "domios_task" ? item.link : null;
  const plan = plans.find((candidate) => linked ? candidate.id === linked.plan_id : item.locked && candidate.tasks.some((task) => task.assigned_tab_id === tabId));
  const task = plan?.tasks.find((candidate) => linked ? candidate.id === linked.task_id : candidate.assigned_tab_id === tabId);
  const taskTarget = linked ? { planId: linked.plan_id, taskId: linked.task_id } : plan && task ? { planId: plan.id, taskId: task.id } : null;
  return <li ref={anchor} data-state={item.state} className="sidebar-agent-checklist__item cursor-pointer"
    role="button" tabIndex={0} aria-expanded={open} aria-controls={open ? detailId : undefined}
    onClick={(event) => { event.stopPropagation(); setOpen((value) => !value); }}
    onKeyDown={(event) => {
      if (event.target !== event.currentTarget) return;
      if (event.code === "Enter" || event.code === "Space") {
        event.preventDefault(); event.stopPropagation(); setOpen(true);
      }
    }}>
    <StateGlyph state={item.state} />
    <span className="sidebar-agent-checklist__title">{item.title}</span>
    <span className="sidebar-agent-checklist__state">{checklistStateLabel(item.state)}</span>
    <AnchoredPopover open={open} anchor={anchor} placement="bottom-start" width={300} onClose={() => setOpen(false)}>
      <section id={detailId} role="region" aria-label="Checklist item details" className="ct-menu p-3 text-xs whitespace-normal break-words space-y-2"
        onClick={(event) => event.stopPropagation()}>
        <p className="font-medium">{item.title}</p>
        {item.description && <p>{item.description}</p>}
        <p>{checklistStateLabel(item.state)} · {item.locked ? "Task criterion (locked)" : "Agent item"}</p>
        {item.state === "in_progress" && item.activeForm && <p>{item.activeForm}</p>}
        {item.reason && <p>Reason: {item.reason}</p>}
        {item.link?.kind === "url" && <a className="text-[var(--ct-accent)] underline" href={item.link.url} target="_blank" rel="noopener noreferrer">{item.link.label} · {item.link.url}</a>}
        {taskTarget && <ChecklistTaskLink target={taskTarget} label={task ? `${task.key ?? task.id} · ${task.name}` : linked?.label ?? ""} onOpen={() => setOpen(false)} />}
      </section>
    </AnchoredPopover>
  </li>;
}

function AgentChecklistRegion({ id, tabId, title, plans }: { id: string; tabId: string; title: string; plans: readonly Plan[] }) {
  const view = useAgentChecklist(tabId);
  const items = view.status === "ready" ? view.checklist?.items ?? [] : [];
  return (
    <section id={id} role="region" aria-label={`Checklist of ${title}`} className="sidebar-agent-checklist">
      {view.status === "loading" ? (
        <p className="sidebar-agent-checklist__note">Loading checklist</p>
      ) : view.status === "error" ? (
        <p className="sidebar-agent-checklist__note">Checklist unavailable</p>
      ) : items.length === 0 ? (
        <p className="sidebar-agent-checklist__note">No checklist</p>
      ) : (
        <ol className="sidebar-agent-checklist__items">
          {items.map((item) => (
            <SidebarAgentChecklist key={item.id} item={item} plans={plans} tabId={tabId} />
          ))}
        </ol>
      )}
    </section>
  );
}

export function AgentChecklistDisclosure({ tabId, title, children, plans = [] }: {
  tabId: string;
  title: string;
  plans?: readonly Plan[];
  children: (toggle: ChecklistToggle, detail: ReactNode) => ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  const controlsId = useId();
  const onToggle = useCallback(() => setExpanded((open) => !open), []);
  const toggle = useMemo(() => ({ expanded, controlsId, onToggle }), [expanded, controlsId, onToggle]);
  const detail = expanded ? <AgentChecklistRegion key={tabId} id={controlsId} tabId={tabId} title={title} plans={plans} /> : null;
  return <>{children(toggle, detail)}</>;
}

export function ChecklistToggleButton({ toggle, title }: { toggle: ChecklistToggle; title: string }) {
  const label = `Checklist for ${title}`;
  return (
    <button
      type="button"
      aria-expanded={toggle.expanded}
      aria-controls={toggle.controlsId}
      aria-label={label}
      title={label}
      data-expanded={toggle.expanded ? "true" : "false"}
      className="sidebar-checklist-toggle"
      onClick={(event) => {
        event.stopPropagation();
        toggle.onToggle();
      }}
    >
      <ListChecks size={11} aria-hidden="true" />
    </button>
  );
}
