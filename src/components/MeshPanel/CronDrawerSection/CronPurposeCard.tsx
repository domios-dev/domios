import { Target } from "lucide-react";
import type { CronJob } from "../../../lib/types";
import { CRON_LABEL, CronCard } from "./parts";

function Fact({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return <div><dt>{label}</dt><dd className={mono ? "ct-detail__mono" : undefined}>{value}</dd></div>;
}

export function CronPurposeCard({ job }: { job: CronJob }) {
  const description = job.description?.trim();
  return <CronCard label="Purpose" icon={<Target size={14} />}>
    <p className="ct-detail__prose m-0 whitespace-pre-wrap break-words">{description || job.task}</p>
    {description && <div className="flex flex-col gap-1.5">
      <h4 className={CRON_LABEL}>Agent task</h4>
      <p className="ct-job-quote">{job.task}</p>
    </div>}
    <dl className="ct-detail__lane">
      <Fact label="Reports to" value={job.report_target_tab_id ? `Tab ${job.report_target_tab_id}` : "Private worker"} />
      <Fact label="Runs" value={job.oneshot ? "Once" : "Recurring"} />
      <Fact label="Agent" value={job.model ? `${job.agent} · ${job.model}` : job.agent} mono />
      <Fact label="Directory" value={job.cwd ?? "Workspace directory"} mono={job.cwd !== null} />
    </dl>
  </CronCard>;
}
