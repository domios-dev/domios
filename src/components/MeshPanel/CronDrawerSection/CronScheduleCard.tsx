import type { CSSProperties } from "react";
import { CalendarClock, Pause, Pencil, Play, Square } from "lucide-react";
import { describeSchedule, formatFuture, formatTime } from "../../../lib/formatters";
import type { InventoryOperation, Service } from "../types";
import { CRON_DANGER, CRON_DANGER_TONE, CRON_LABEL, CRON_PILL, CRON_PRIMARY, CronCard, runStatusLabel, Unavailable } from "./parts";

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

function Status({ enabled, live }: { enabled: boolean; live: string | null }) {
  const [label, tone] = live ? [live, "var(--ct-accent)"] : enabled ? ["Active", "var(--ct-green)"] : ["Paused", "var(--ct-muted)"];
  return <span className="ct-detail__status shrink-0 text-xs" style={{ "--ct-detail-tone": tone } as CSSProperties}>
    <span className="ct-detail__dot" aria-hidden="true" />{label}
  </span>;
}

export function CronScheduleCard(props: CronScheduleCardProps) {
  const { service, busy, running, editing, scheduleInput, toggleOperation, can, reason } = props;
  const nextRun = service.next_run ? `${formatFuture(service.next_run)} · ${formatTime(service.next_run)}` : "Not scheduled";
  return <CronCard label="Schedule" icon={<CalendarClock size={14} />} aside={<Status enabled={service.enabled} live={running && service.last_run_status ? runStatusLabel(service.last_run_status) : null} />}>
    <p className="ct-job-section__lead">{service.schedule ? describeSchedule(service.schedule) : "Schedule not set"}</p>
    <dl className="ct-detail__lane">
      <div><dt>Next run</dt><dd>{nextRun}</dd></div>
      <div><dt>Last run</dt><dd>{service.last_run_status ? runStatusLabel(service.last_run_status) : "Not recorded"}</dd></div>
      {!editing && <div><dt>Expression</dt><dd className="flex flex-wrap items-center gap-2">
        <code className="ct-detail__mono">{service.schedule ?? "—"}</code>
        <button type="button" onClick={props.onEdit} disabled={busy || !can("update_schedule")} className={CRON_PILL} title={reason("update_schedule") ?? "Edit schedule"}><Pencil size={12} aria-hidden="true" />Edit</button>
      </dd></div>}
    </dl>
    {editing && <div className="flex flex-col gap-2">
      <label className={CRON_LABEL} htmlFor={`cron-schedule-${service.id}`}>Cron expression</label>
      <div className="ct-job-actions">
        <input id={`cron-schedule-${service.id}`} value={scheduleInput} onChange={(event) => props.onScheduleInput(event.target.value)} className="ct-field min-w-0 flex-1 font-mono" placeholder="*/30 * * * *" aria-label="Schedule expression" />
        <button type="button" onClick={props.onSave} disabled={busy || !can("update_schedule")} className={CRON_PRIMARY}>Save</button>
        <button type="button" onClick={props.onCancelEdit} className={CRON_PILL} aria-label="Cancel schedule edit">Cancel</button>
      </div>
    </div>}
    {!can("update_schedule") && <Unavailable operation="update_schedule" reason={reason("update_schedule")} />}
    <div role="group" aria-label="Schedule controls" className="ct-job-actions">
      <button type="button" onClick={props.onTrigger} disabled={busy || !can("trigger")} className={CRON_PRIMARY} title={reason("trigger")}><Play size={12} aria-hidden="true" />Trigger now</button>
      <button type="button" onClick={props.onToggle} disabled={busy || !can(toggleOperation)} className={CRON_PILL} title={reason(toggleOperation)}>{service.enabled ? <Pause size={12} aria-hidden="true" /> : <Play size={12} aria-hidden="true" />}{service.enabled ? "Pause" : "Resume"}</button>
      {running && <button type="button" onClick={props.onCancelRun} disabled={busy || !can("cancel")} className={CRON_DANGER} style={CRON_DANGER_TONE} title={reason("cancel")}><Square size={11} aria-hidden="true" />Cancel running</button>}
    </div>
    {!can("trigger") && <Unavailable operation="trigger" reason={reason("trigger")} />}
    {!can(toggleOperation) && <Unavailable operation={toggleOperation} reason={reason(toggleOperation)} />}
    {running && !can("cancel") && <Unavailable operation="cancel" reason={reason("cancel")} />}
  </CronCard>;
}
