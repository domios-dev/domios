import { useMemo, useState } from "react";
import type { DsTasksBlock } from "@codeterm/chat-ui/dsTasksBlock";
import { useSidebarPlansOptional } from "../../Canvas/PlansProvider";
import { TASK_STATUS_COLORS } from "../../Canvas/settings";
import { usePlanTargetNavigation } from "../../Canvas/teamViewNavigation";
import type { TaskStatus } from "../../Canvas/types";
import { formatModelBadge } from "../../../lib/formatModelBadge";
import {
  dsTaskProgressLabel,
  dsTaskRowsForBlock,
  groupDsTaskRows,
  type DsTaskGroup,
  type DsTaskRow,
} from "./dsTasksModel";

function statusColor(status: string): string {
  return TASK_STATUS_COLORS[status as TaskStatus] ?? "var(--ct-muted-fg)";
}

function TaskRow({ row, onOpen }: { row: DsTaskRow; onOpen: (taskId: string) => void }) {
  const status = row.status.replace(/_/g, " ");
  return (
    <li>
      <button
        type="button"
        onClick={() => onOpen(row.id)}
        aria-label={`Open ${row.key} ${row.name} in the plan`}
        className="grid w-full grid-cols-[auto_minmax(0,1fr)_auto] items-baseline gap-x-3 gap-y-0.5 rounded-[var(--ct-radius-sm)] px-2 py-1.5 text-left hover:bg-[var(--ct-bg-hover)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-border-focus)]"
        style={{ color: "var(--ct-fg)", border: "none", cursor: "pointer" }}
      >
        <span className="ct-text-mono text-[11px] tabular-nums text-[var(--ct-muted-fg)]">{row.key}</span>
        <span className="min-w-0 truncate text-[13px]">{row.name || row.key}</span>
        <span className="ct-small-pill shrink-0" style={{ color: "var(--ct-fg)" }}>
          <span aria-hidden className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: statusColor(row.status) }} />
          {status}
        </span>
        <span className="col-start-2 col-end-4 flex min-w-0 flex-wrap gap-x-3 text-[11px] text-[var(--ct-muted-fg)]">
          <span className="min-w-0 truncate">
            {row.detached ? "Not in plan data" : row.executor ?? "Unassigned"}
          </span>
          {row.model && <span className="min-w-0 truncate">{formatModelBadge(row.model)}</span>}
          {!row.detached && <span>{dsTaskProgressLabel(row)}</span>}
        </span>
      </button>
    </li>
  );
}

function TaskGroup({ group, onOpen }: { group: DsTaskGroup; onOpen: (taskId: string) => void }) {
  const [open, setOpen] = useState(!group.collapsedByDefault);
  const heading = (
    <>
      <span>{group.label}</span>
      <span className="tabular-nums opacity-70">{group.rows.length}</span>
    </>
  );
  const headingClass = "flex w-full items-baseline gap-2 px-2 pt-2 pb-1 text-left text-[11px] font-medium text-[var(--ct-muted-fg)]";
  return (
    <section aria-label={`${group.label}, ${group.rows.length}`} className="min-w-0">
      {group.collapsedByDefault ? (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
          className={`${headingClass} hover:text-[var(--ct-fg)]`}
          style={{ border: "none", background: "transparent", cursor: "pointer" }}
        >
          {heading}
          <span className="ml-auto opacity-70">{open ? "Hide" : "Show"}</span>
        </button>
      ) : (
        <div className={headingClass}>{heading}</div>
      )}
      {open && (
        <ul className="m-0 grid list-none gap-0.5 p-0">
          {group.rows.map((row) => <TaskRow key={row.id} row={row} onOpen={onOpen} />)}
        </ul>
      )}
    </section>
  );
}

/** Live plan tasks for a `<ds-tasks>` chat block; plan events re-render it through the plans context. */
export function DsTasksChatBlock({ block }: { block: DsTasksBlock }) {
  const { plans } = useSidebarPlansOptional();
  const openPlanTarget = usePlanTargetNavigation();
  const plan = plans.find((candidate) => candidate.id === block.planId);
  const groups = useMemo(() => groupDsTaskRows(dsTaskRowsForBlock(block, plan)), [block, plan]);
  const title = plan?.title ?? block.planTitle;
  const openTask = (taskId: string) => openPlanTarget({ planId: block.planId, taskId, teamId: plan?.team_id });

  return (
    <div
      data-testid="chat-ds-tasks-block"
      className="ct-panel-card w-full max-w-xl min-w-0 px-1.5 pt-1.5 pb-2"
      style={{ background: "var(--ct-surface, var(--ct-bg))", color: "var(--ct-fg)" }}
    >
      <button
        type="button"
        onClick={() => openPlanTarget({ planId: block.planId, teamId: plan?.team_id })}
        className="flex w-full min-w-0 items-baseline gap-2 px-2 py-1 text-left hover:text-[var(--ct-accent)]"
        style={{ border: "none", background: "transparent", color: "var(--ct-fg)", cursor: "pointer" }}
      >
        <span className="min-w-0 truncate text-[13px] font-semibold">{title}</span>
        <span className="shrink-0 text-[11px] tabular-nums text-[var(--ct-muted-fg)]">
          {block.items.length} {block.items.length === 1 ? "task" : "tasks"}
        </span>
      </button>
      {groups.length === 0 ? (
        <p className="m-0 px-2 py-1 text-[12px] text-[var(--ct-muted-fg)]">No tasks match this mention.</p>
      ) : (
        groups.map((group) => <TaskGroup key={group.key} group={group} onOpen={openTask} />)
      )}
    </div>
  );
}
