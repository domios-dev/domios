import { InlineErrorNotice } from "../shared/ErrorNotice";
import { probeStateLabel } from "../Automation/probeProgress";
import { useState, type CSSProperties, type ReactNode } from "react";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { describeSchedule, formatFuture } from "../../lib/formatters";
import { formatDuration } from "../../lib/timeLabels";
import type { CronJob } from "../../lib/types";
import type { BlueprintScenarioJob } from "../Automation/scenarios";
import { AUTOMATION_ROW, AUTOMATION_TONE, stateTone } from "../Automation/styles";
import { DropdownMenu } from "../shared/DropdownMenu";
import { RowMenuButton } from "../shared/RowMenuButton";

// The stretched ::after makes the whole card open the job while the menu sits above it.
const JOB_OPEN = "flex min-w-0 flex-1 items-center gap-3 text-left outline-none after:absolute after:inset-0 after:rounded-[14px] focus-visible:after:outline-2 focus-visible:after:outline-offset-2 focus-visible:after:outline-[var(--ct-accent)]";

export interface JobAction {
  label: string;
  icon?: ReactNode;
  disabled?: boolean;
  danger?: boolean;
  onClick: () => void;
}

export function JobRow({ jobKey: key, icon, name, meta, idHint, type, tone, state, actionItems, onOpen, onTriggerRef, error }: {
  jobKey: string;
  icon: ReactNode;
  name: string;
  meta: string;
  idHint?: string;
  type: string;
  tone: keyof typeof AUTOMATION_TONE;
  state: string;
  actionItems: JobAction[];
  onOpen: () => void;
  onTriggerRef: (element: HTMLButtonElement | null) => void;
  error: string | null;
}) {
  const actionsLabel = `Actions for ${name}`;
  const touchMode = useTouchOptimizedUi();
  const [menuOpen, setMenuOpen] = useState(false);
  const reveal = touchMode || menuOpen ? "" : "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100";
  return <li className="min-w-0">
    <div className={`${AUTOMATION_ROW} relative`} style={{ "--ct-card-tone": AUTOMATION_TONE[tone] } as CSSProperties}>
      <button type="button" data-job-key={key} aria-label={`${name}, ${type}, ${state}`} title={idHint} className={JOB_OPEN} onClick={onOpen}>
        <span className="ct-card__icon" title={type}>{icon}</span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[length:var(--ct-font-size-sm)] font-medium">{name}</span>
          <span className="block truncate text-xs text-[var(--ct-muted-fg)]">{meta}</span>
        </span>
        <span className="flex shrink-0 items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(state)}`} aria-hidden="true" />{state}</span>
      </button>
      <DropdownMenu
        menuLabel={actionsLabel}
        items={actionItems}
        onOpenChange={setMenuOpen}
        className={`relative z-[1] flex shrink-0 items-center transition-opacity ${reveal}`}
        trigger={<RowMenuButton ref={onTriggerRef} label={actionsLabel} touchMode={touchMode} />}
      />
    </div>
    {error && <InlineErrorNotice error={error} operation="updating this job" blocking />}
  </li>;
}

export function cronMeta(job: CronJob): string {
  const schedule = describeSchedule(job.schedule);
  const next = job.enabled && job.next_run ? `${schedule} · next ${formatFuture(job.next_run)}` : schedule;
  return `${next} · last run: ${job.last_run?.status ?? "none"}`;
}

/** A Blueprint row's secondary line describes what it does; ids live only in the hover hint. */
export function sentinelRowText(job: BlueprintScenarioJob, title: string): { meta: string; idHint: string } {
  const { root, scenario } = job;
  const purpose = root?.purpose?.trim();
  const schedule = root?.schedule?.trim();
  const meta = purpose && purpose !== title ? purpose
    : schedule ? describeSchedule(schedule)
    : root?.cadence_ms ? `Every ${formatDuration(root.cadence_ms)}`
    : "No description";
  return { meta, idHint: `Blueprint ${scenario.id}` };
}

export function sentinelStateLabel(job: BlueprintScenarioJob): string {
  return job.root ? probeStateLabel(job.root) : job.state;
}
