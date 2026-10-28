import { useCallback, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { Circle, CircleAlert, CircleCheck, CircleDot, CircleMinus, ListChecks } from "lucide-react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { BusyState } from "../shared/BusyState";
import { ChecklistItemDetail } from "./ChecklistItemDetail";
import type { Plan } from "../Canvas/types";
import { useAgentChecklist } from "./useAgentChecklists";
import {
  checklistStateLabel,
  type AgentChecklistItem,
  type AgentChecklistState,
} from "./agentChecklistModel";

export interface ChecklistToggle {
  expanded: boolean;
  controlsId: string;
  onToggle: () => void;
}

function StateGlyph({ state }: { state: AgentChecklistState }) {
  const Icon = state === "done" ? CircleCheck
    : state === "in_progress" ? CircleDot
    : state === "blocked" ? CircleAlert
    : state === "skipped" ? CircleMinus
    : Circle;
  return <Icon size={10} strokeWidth={1.75} aria-hidden="true" className="sidebar-agent-checklist__glyph" />;
}

function SidebarAgentChecklist({ item, plans, tabId }: { item: AgentChecklistItem; plans: readonly Plan[]; tabId: string }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLLIElement>(null);
  const detailId = useId();
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
    <span className="sidebar-agent-checklist__title" title={item.reason ? `${item.title} — ${item.reason}` : item.title}>{item.title}</span>
    <span className="sidebar-agent-checklist__state">{checklistStateLabel(item.state)}</span>
    <AnchoredPopover open={open} anchor={anchor} placement="bottom-start" width={300} onClose={() => setOpen(false)}>
      <ChecklistItemDetail id={detailId} item={item} plans={plans} tabId={tabId} onClose={() => setOpen(false)} />
    </AnchoredPopover>
  </li>;
}

function AgentChecklistRegion({ id, tabId, title, plans }: { id: string; tabId: string; title: string; plans: readonly Plan[] }) {
  const view = useAgentChecklist(tabId);
  const items = view.status === "ready" ? view.checklist?.items ?? [] : [];
  return (
    <section id={id} role="region" aria-label={`Checklist of ${title}`} className="sidebar-agent-checklist">
      {view.status === "ready" && view.checklist?.proposal?.state === "proposed" && <p role="status" className="sidebar-agent-checklist__note">Not reviewed yet</p>}
      {view.status === "loading" ? (
        <BusyState inline label="Loading checklist" phase="work" className="sidebar-agent-checklist__note" />
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
