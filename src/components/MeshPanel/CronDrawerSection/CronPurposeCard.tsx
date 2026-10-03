import type { CronJob } from "../../../lib/types";
import { CRON_LABEL, CRON_TEXT, CronCard } from "./parts";

function Fact({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return <div className="min-w-0">
    <dt className="text-xs text-[var(--ct-muted)]">{label}</dt>
    <dd className={`mt-0.5 break-words text-[var(--ct-fg)] ${mono ? "font-mono text-xs" : CRON_TEXT}`}>{value}</dd>
  </div>;
}

export function CronPurposeCard({ job }: { job: CronJob }) {
  const description = job.description?.trim();
  return <CronCard label="Purpose">
    <p className="whitespace-pre-wrap break-words text-[length:var(--ct-font-size-md)] leading-relaxed text-[var(--ct-fg)]">{description || job.task}</p>
    {description && <div>
      <h4 className={CRON_LABEL}>Agent task</h4>
      <p className={`mt-1.5 max-h-60 overflow-y-auto whitespace-pre-wrap break-words rounded-md border-l-2 border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-fg)_4%,transparent)] px-3 py-2 text-[var(--ct-muted-fg)] ${CRON_TEXT}`}>{job.task}</p>
    </div>}
    <dl className="grid grid-cols-[repeat(auto-fill,minmax(9.5rem,1fr))] gap-x-6 gap-y-3 border-t border-[var(--ct-border)] pt-3">
      <Fact label="Reports to" value={job.report_target_tab_id ? `Tab ${job.report_target_tab_id}` : "Private worker"} />
      <Fact label="Action" value="Start agent" />
      <Fact label="Runs" value={job.oneshot ? "Once" : "Recurring"} />
      <Fact label="Directory" value={job.cwd ?? "Workspace directory"} mono={job.cwd !== null} />
      <Fact label="Agent provider" value={job.agent} mono />
      <Fact label="Model" value={job.model ?? "default"} mono />
    </dl>
  </CronCard>;
}
