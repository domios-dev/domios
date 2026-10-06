import type { ReactNode } from "react";
import { ChevronRight, MoreHorizontal } from "lucide-react";
import { describeSchedule, formatFuture } from "../../lib/formatters";
import type { CronJob } from "../../lib/types";
import type { SentinelJob } from "../Automation/scenarios";
import { AUTOMATION_ROW, AUTOMATION_TOOL, stateTone } from "../Automation/styles";
import { DropdownMenu } from "../shared/DropdownMenu";

export interface JobAction {
  label: string;
  icon?: ReactNode;
  disabled?: boolean;
  danger?: boolean;
  onClick: () => void;
}

export function JobRow({ jobKey: key, icon, name, meta, detail, type, state, actionItems, onOpen, onTriggerRef, error }: {
  jobKey: string;
  icon: ReactNode;
  name: string;
  meta: string;
  detail?: string;
  type: string;
  state: string;
  actionItems: JobAction[];
  onOpen: () => void;
  onTriggerRef: (element: HTMLButtonElement | null) => void;
  error: string | null;
}) {
  const actionsLabel = `Actions for ${name}`;
  return <li className="min-w-0">
    <div className="flex min-w-0 items-stretch gap-1">
      <button type="button" data-job-key={key} aria-label={`${name}, ${type}, ${state}`} className={`${AUTOMATION_ROW} flex-1`} onClick={onOpen}>
        <span className="grid h-8 w-8 shrink-0 place-items-center rounded-md bg-[var(--ct-titlebar-bg)] text-[var(--ct-muted-fg)]">{icon}</span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[length:var(--ct-font-size-sm)] font-medium">{name}</span>
          <span className="block truncate text-xs text-[var(--ct-muted-fg)]">{meta}{detail && <span className="text-[var(--ct-muted)]"> · {detail}</span>}</span>
        </span>
        <span className="flex shrink-0 flex-col items-end gap-1">
          <span className="rounded-full border border-[var(--ct-border)] px-2 py-px text-[0.6875rem] text-[var(--ct-muted-fg)]">{type}</span>
          <span className="flex items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(state)}`} aria-hidden="true" />{state}</span>
        </span>
        <ChevronRight size={14} className="shrink-0 text-[var(--ct-muted)]" aria-hidden="true" />
      </button>
      <DropdownMenu
        menuLabel={actionsLabel}
        items={actionItems}
        className="flex shrink-0 items-center"
        trigger={<button ref={onTriggerRef} type="button" className={`${AUTOMATION_TOOL} h-full min-w-9 px-2`} aria-label={actionsLabel} aria-haspopup="menu" title={actionsLabel}><MoreHorizontal size={16} aria-hidden="true" /></button>}
      />
    </div>
    {error && <p role="alert" className="px-3 pt-1 text-xs text-[var(--ct-fg)]">{error}</p>}
  </li>;
}

export function cronMeta(job: CronJob): string {
  const schedule = describeSchedule(job.schedule);
  const next = job.enabled && job.next_run ? `${schedule} · next ${formatFuture(job.next_run)}` : schedule;
  return `${next} · last run: ${job.last_run?.status ?? "none"}`;
}

export function unnamedGraphId(job: SentinelJob, title: string): string | undefined {
  const { name, id } = job.scenario;
  return (!name.trim() || name === id) && title !== id ? id : undefined;
}

export function sentinelMeta(job: SentinelJob): string {
  return job.scenario.name === job.scenario.id ? job.scenario.root.id : job.root?.purpose ?? job.scenario.root.id;
}
