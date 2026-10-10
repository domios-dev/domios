import { useState } from "react";
import { ListTodo } from "lucide-react";
import { ChatBlockFrame } from "./ChatBlockFrame";
import { useChatBlockHost, type TasksBlockHost } from "./ChatBlockHost";
import { useLiveBinding } from "./liveBinding";
import { BlockNotice } from "./BlockNotice";
import {
  tasksBlockKey,
  tasksBlockSummary,
  type TasksBlockGroup,
  type TasksBlockRow,
  type TasksBlockSpec,
  type TasksBlockState,
} from "./tasksBlockModel";

const LOADING: TasksBlockState = { kind: "loading" };
const FOCUS_RING = "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--ct-accent)]";

/** `@ds-tasks <plan-id> …` and a sent `<ds-tasks>` message: live plan tasks grouped by status. */
export function TasksBlock({ spec, source }: { spec: TasksBlockSpec; source: string }) {
  const host = useChatBlockHost().tasks;
  const state = useLiveBinding(host?.binding, tasksBlockKey(spec), LOADING);
  const label = `Tasks of ${spec.planTitle ?? spec.planId}`;
  if (!host) return <BlockNotice name="tasks" source={source} label={label} text="Plan tasks are unavailable in this view." icon={ListTodo} />;
  switch (state.kind) {
    case "loading":
      return <BlockNotice name="tasks" source={source} label={label} text="Reading the plan…" icon={ListTodo} />;
    case "missing":
      return <BlockNotice name="tasks" source={source} label={label} text={`Plan ${spec.planId} is not available.`} icon={ListTodo} missing />;
    case "ready":
      return <TasksCard state={state} spec={spec} host={host} source={source} />;
  }
}

function TasksCard({ state, spec, host, source }: { state: Extract<TasksBlockState, { kind: "ready" }>; spec: TasksBlockSpec; host: TasksBlockHost; source: string }) {
  const openTask = (taskId: string) => host.openTask(spec.planId, taskId);
  return (
    <ChatBlockFrame
      name="tasks"
      source={source}
      title={state.title}
      icon={<ListTodo size={14} aria-hidden="true" />}
      status={{ label: `${state.total} ${state.total === 1 ? "task" : "tasks"}`, tone: "accent" }}
      summary={tasksBlockSummary(state)}
      body={state.groups.length === 0
        ? <p className="m-0 text-xs" style={{ color: "var(--ct-muted-fg)" }}>No tasks match this block.</p>
        : <div className="-mx-1.5 grid gap-1">{state.groups.map((group) => <TaskGroup key={group.key} group={group} onOpen={openTask} />)}</div>}
      fullPage={{ label: "Open plan", open: () => host.openPlan(spec.planId) }}
      askText={`About these tasks (\`${source}\`): `}
    />
  );
}

function TaskGroup({ group, onOpen }: { group: TasksBlockGroup; onOpen: (taskId: string) => void }) {
  const [open, setOpen] = useState(!group.collapsedByDefault);
  const heading = (
    <>
      <span>{group.label}</span>
      <span className="tabular-nums opacity-70">{group.rows.length}</span>
    </>
  );
  const headingClass = "flex w-full items-baseline gap-2 px-1.5 pt-1 pb-0.5 text-left text-[11px] font-medium";
  return (
    <section aria-label={`${group.label}, ${group.rows.length}`} className="min-w-0" data-task-group={group.key}>
      {group.collapsedByDefault ? (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
          className={`${headingClass} rounded hover:text-[var(--ct-fg)] ${FOCUS_RING}`}
          style={{ color: "var(--ct-muted-fg)" }}
        >
          {heading}
          <span className="ml-auto opacity-70">{open ? "Hide" : "Show"}</span>
        </button>
      ) : (
        <div className={headingClass} style={{ color: "var(--ct-muted-fg)" }}>{heading}</div>
      )}
      {open && (
        <ul className="m-0 grid list-none gap-0.5 p-0">
          {group.rows.map((row) => <TaskRow key={row.id} row={row} onOpen={onOpen} />)}
        </ul>
      )}
    </section>
  );
}

function TaskRow({ row, onOpen }: { row: TasksBlockRow; onOpen: (taskId: string) => void }) {
  const status = row.status.replace(/_/g, " ");
  return (
    <li>
      <button
        type="button"
        data-task-row={row.id}
        onClick={() => onOpen(row.id)}
        aria-label={`Open ${row.key} ${row.name} in the plan`}
        className={`grid w-full grid-cols-[auto_minmax(0,1fr)_auto] items-baseline gap-x-3 gap-y-0.5 px-1.5 py-1.5 text-left ct-row ${FOCUS_RING}`}
        style={{ color: "var(--ct-fg)" }}
      >
        <span className="text-[11px] tabular-nums" style={{ color: "var(--ct-muted-fg)" }}>{row.key}</span>
        <span className="min-w-0 truncate text-[13px]">{row.name || row.key}</span>
        <span className="inline-flex shrink-0 items-center gap-1.5 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
          <span aria-hidden="true" className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: row.color ?? "var(--ct-muted-fg)" }} />
          {status}
        </span>
        <span className="col-start-2 col-end-4 flex min-w-0 flex-wrap gap-x-3 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
          <span className="min-w-0 truncate">{row.detached ? "Not in plan data" : row.executor ?? "Unassigned"}</span>
          {row.model && <span className="min-w-0 truncate">{row.model}</span>}
          {row.progress && <span>{row.progress}</span>}
        </span>
      </button>
    </li>
  );
}
