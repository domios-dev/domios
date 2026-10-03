import { Pause, Pencil, Play, Square } from "lucide-react";
import { describeSchedule, formatFuture, formatTime } from "../../../lib/formatters";
import { UI_INPUT } from "../../shared/design";
import type { InventoryOperation, Service } from "../types";
import { CRON_DANGER, CRON_LABEL, CRON_PRIMARY, CRON_QUIET, CRON_SECONDARY, CRON_TEXT, CronCard, runStatusLabel, Unavailable } from "./parts";

export interface CronScheduleCardProps {
  service: Service;
  busy: boolean;
  running: boolean;
  editing: boolean;
  scheduleInput: string;
  toggleOperation: InventoryOperation;
  can: (operation: InventoryOperation) => boolean;
  reason: (operation: InventoryOperation) => string | undefined;
  onScheduleInput: (value: string) => void;
  onEdit: () => void;
  onCancelEdit: () => void;
  onSave: () => void;
  onTrigger: () => void;
  onToggle: () => void;
  onCancelRun: () => void;
}

function StatusBadge({ enabled, live }: { enabled: boolean; live: string | null }) {
  const [label, tone] = live ? [live, "var(--ct-accent)"] : enabled ? ["Active", "var(--ct-green)"] : ["Paused", "var(--ct-muted)"];
  return <span className="inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs font-medium" style={{ color: tone, borderColor: `color-mix(in srgb, ${tone} 40%, transparent)` }}>
    <span className="h-1.5 w-1.5 rounded-full" style={{ background: tone }} aria-hidden="true" />{label}
  </span>;
}

export function CronScheduleCard(props: CronScheduleCardProps) {
  const { service, busy, running, editing, scheduleInput, toggleOperation, can, reason } = props;
  const nextRun = service.next_run ? `${formatFuture(service.next_run)} · ${formatTime(service.next_run)}` : "Not scheduled";
  return <CronCard label="Schedule and controls">
    <div className="flex flex-wrap items-start gap-x-4 gap-y-3">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2.5">
          <StatusBadge enabled={service.enabled} live={running && service.last_run_status ? runStatusLabel(service.last_run_status) : null} />
          <span className="text-[length:var(--ct-font-size-lg)] font-semibold text-[var(--ct-fg)]">{service.schedule ? describeSchedule(service.schedule) : "Schedule not set"}</span>
        </div>
        <dl className={`mt-3 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5 ${CRON_TEXT}`}>
          <dt className="text-[var(--ct-muted)]">Expression</dt>
          <dd><code className="rounded bg-[color-mix(in_srgb,var(--ct-fg)_6%,transparent)] px-1.5 py-0.5 font-mono text-xs text-[var(--ct-fg)]">{service.schedule ?? "—"}</code></dd>
          <dt className="text-[var(--ct-muted)]">Next run</dt>
          <dd className="text-[var(--ct-fg)]">{nextRun}</dd>
          <dt className="text-[var(--ct-muted)]">Last run</dt>
          <dd className="text-[var(--ct-fg)]">{service.last_run_status ? runStatusLabel(service.last_run_status) : "Not recorded"}</dd>
        </dl>
      </div>
      {!editing && <button type="button" onClick={props.onEdit} disabled={busy || !can("update_schedule")} className={CRON_QUIET} title={reason("update_schedule") ?? "Edit schedule"}><Pencil size={13} aria-hidden="true" />Edit</button>}
    </div>
    {editing && <div className="flex flex-col gap-2">
      <label className={CRON_LABEL} htmlFor={`cron-schedule-${service.id}`}>Cron expression</label>
      <div className="flex flex-wrap gap-2">
        <input id={`cron-schedule-${service.id}`} value={scheduleInput} onChange={(event) => props.onScheduleInput(event.target.value)} className={`${UI_INPUT.base} min-w-0 flex-1 px-2.5 py-1.5 font-mono text-[length:var(--ct-font-size-sm)] outline-none focus:border-[var(--ct-accent)]`} placeholder="*/30 * * * *" aria-label="Schedule expression" />
        <button type="button" onClick={props.onSave} disabled={busy || !can("update_schedule")} className={CRON_PRIMARY}>Save</button>
        <button type="button" onClick={props.onCancelEdit} className={CRON_SECONDARY} aria-label="Cancel schedule edit">Cancel</button>
      </div>
    </div>}
    {!can("update_schedule") && <Unavailable operation="update_schedule" reason={reason("update_schedule")} />}
    <div role="group" aria-label="Schedule controls" className="flex flex-wrap items-center gap-2 border-t border-[var(--ct-border)] pt-3">
      <button type="button" onClick={props.onTrigger} disabled={busy || !can("trigger")} className={CRON_PRIMARY} title={reason("trigger")}><Play size={13} aria-hidden="true" />Trigger now</button>
      <button type="button" onClick={props.onToggle} disabled={busy || !can(toggleOperation)} className={CRON_SECONDARY} title={reason(toggleOperation)}>{service.enabled ? <Pause size={13} aria-hidden="true" /> : <Play size={13} aria-hidden="true" />}{service.enabled ? "Pause" : "Resume"}</button>
      {running && <button type="button" onClick={props.onCancelRun} disabled={busy || !can("cancel")} className={CRON_DANGER} title={reason("cancel")}><Square size={12} aria-hidden="true" />Cancel running</button>}
    </div>
    {!can("trigger") && <Unavailable operation="trigger" reason={reason("trigger")} />}
    {!can(toggleOperation) && <Unavailable operation={toggleOperation} reason={reason(toggleOperation)} />}
    {running && !can("cancel") && <Unavailable operation="cancel" reason={reason("cancel")} />}
  </CronCard>;
}
