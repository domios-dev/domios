import { useCallback, useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { Circle, CircleAlert, CircleCheck, CircleDot, CircleMinus, ListChecks } from "lucide-react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import {
  boundedChecklist,
  checklistStateLabel,
  eventNeedsReread,
  isTabTodoChanged,
  TAB_TODO_CHANGED_EVENT,
  type WorkerChecklist,
  type WorkerChecklistState,
} from "./workerChecklistModel";

export interface ChecklistToggle {
  expanded: boolean;
  controlsId: string;
  onToggle: () => void;
}

type ChecklistView =
  | { status: "loading" }
  | { status: "error" }
  | { status: "ready"; checklist: WorkerChecklist | null };

/** Mounted only while the detail is open, so the subscription never outlives the disclosure. */
function useWorkerChecklist(tabId: string): ChecklistView {
  const [view, setView] = useState<ChecklistView>({ status: "loading" });
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    let shown: WorkerChecklist | null = null;
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

function StateGlyph({ state }: { state: WorkerChecklistState }) {
  const Icon = state === "done" ? CircleCheck
    : state === "in_progress" ? CircleDot
    : state === "blocked" ? CircleAlert
    : state === "skipped" ? CircleMinus
    : Circle;
  return <Icon size={10} strokeWidth={1.75} aria-hidden="true" className="sidebar-worker-checklist__glyph" />;
}

function WorkerChecklistRegion({ id, tabId }: { id: string; tabId: string }) {
  const view = useWorkerChecklist(tabId);
  const items = view.status === "ready" ? view.checklist?.items ?? [] : [];
  return (
    <section id={id} role="region" aria-label="Worker checklist" className="sidebar-worker-checklist">
      {view.status === "loading" ? (
        <p className="sidebar-worker-checklist__note">Loading checklist</p>
      ) : view.status === "error" ? (
        <p className="sidebar-worker-checklist__note">Checklist unavailable</p>
      ) : items.length === 0 ? (
        <p className="sidebar-worker-checklist__note">No checklist</p>
      ) : (
        <ol className="sidebar-worker-checklist__items">
          {items.map((item) => (
            <li key={item.id} data-state={item.state} className="sidebar-worker-checklist__item">
              <StateGlyph state={item.state} />
              <span className="sidebar-worker-checklist__title" title={item.reason ? `${item.title} — ${item.reason}` : item.title}>{item.title}</span>
              <span className="sidebar-worker-checklist__state">{checklistStateLabel(item.state)}</span>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

export function WorkerChecklistDisclosure({ tabId, children }: {
  tabId: string;
  children: (toggle: ChecklistToggle, detail: ReactNode) => ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  const controlsId = useId();
  const onToggle = useCallback(() => setExpanded((open) => !open), []);
  const toggle = useMemo(() => ({ expanded, controlsId, onToggle }), [expanded, controlsId, onToggle]);
  const detail = expanded ? <WorkerChecklistRegion key={tabId} id={controlsId} tabId={tabId} /> : null;
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
