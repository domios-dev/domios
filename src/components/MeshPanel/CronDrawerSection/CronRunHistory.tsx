import { MessageSquareText } from "lucide-react";
import { formatTime } from "../../../lib/formatters";
import type { CronRunInfo } from "../../../lib/types";
import { CRON_QUIET, CronCard, formatRunDuration, runColor, runIcon, runStatusLabel, Unavailable } from "./parts";

export function CronRunHistory({ runs, allowed, reason, onOpen }: { runs: readonly CronRunInfo[]; allowed: boolean; reason?: string; onOpen: (run: CronRunInfo) => void }) {
  return <CronCard label="Run history" aside={allowed && runs.length > 0 ? <span className="text-xs text-[var(--ct-muted)]">{runs.length} recent</span> : undefined}>
    {!allowed ? <Unavailable operation="history" reason={reason} />
      : runs.length === 0 ? <p className="text-[length:var(--ct-font-size-sm)] text-[var(--ct-muted-fg)]">No runs yet</p>
      : <ol aria-label="Run history" className="-mx-1.5 flex max-h-72 flex-col overflow-y-auto">{runs.map((run) => {
        const started = formatTime(run.started_at);
        const label = runStatusLabel(run.status);
        return <li key={run.run_id} className="grid grid-cols-[1rem_minmax(0,1fr)_auto_auto] items-center gap-x-3 rounded-md px-1.5 py-1.5 text-[length:var(--ct-font-size-sm)] hover:bg-[var(--ct-hover)]">
          <span className={`text-center ${runColor(run.status)}`} aria-hidden="true">{runIcon(run.status)}</span>
          <span className="min-w-0 truncate"><span className="text-[var(--ct-fg)]">{label}</span><span className="text-[var(--ct-muted-fg)]"> · {started}</span></span>
          <span className="font-mono text-xs text-[var(--ct-muted-fg)]">{run.finished_at ? formatRunDuration(run.finished_at - run.started_at) : ""}</span>
          {run.session_id
            ? <button type="button" className={CRON_QUIET} aria-label={`View transcript: ${label} run, ${started}`} onClick={() => onOpen(run)}><MessageSquareText size={13} aria-hidden="true" />Transcript</button>
            : <span className="px-2 text-xs text-[var(--ct-muted)]">No transcript</span>}
        </li>;
      })}</ol>}
  </CronCard>;
}
