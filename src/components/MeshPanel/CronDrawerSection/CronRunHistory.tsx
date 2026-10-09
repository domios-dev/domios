import type { CSSProperties } from "react";
import { History, MessageSquareText } from "lucide-react";
import { formatTime } from "../../../lib/formatters";
import type { CronRunInfo } from "../../../lib/types";
import { CronCard, formatRunDuration, runStatusLabel, runTone, Unavailable } from "./parts";

export function CronRunHistory({ runs, allowed, reason, onOpen }: { runs: readonly CronRunInfo[]; allowed: boolean; reason?: string; onOpen: (run: CronRunInfo) => void }) {
  const listed = allowed && runs.length > 0;
  return <CronCard label="Run history" icon={<History size={14} />} bodyless={listed} aside={listed ? <span className="ct-card__meta shrink-0">{runs.length} recent</span> : undefined}>
    {!allowed ? <Unavailable operation="history" reason={reason} />
      : runs.length === 0 ? <p className="ct-job-note">No runs yet</p>
      : <ol aria-label="Run history" className="ct-rows">{runs.map((run) => {
        const started = formatTime(run.started_at);
        const label = runStatusLabel(run.status);
        const live = run.status === "Running" || run.status === "AwaitingOwnerPrompt";
        return <li key={run.run_id} className="ct-row ct-run-row" data-run-status={run.status}>
          <span className="ct-detail__dot" data-live={live || undefined} style={{ "--ct-detail-tone": runTone(run.status) } as CSSProperties} aria-hidden="true" />
          <span className="ct-run-row__time">{started}</span>
          <span className="ct-run-row__main">{label}</span>
          <span className="ct-run-row__meta">{run.finished_at ? formatRunDuration(run.finished_at - run.started_at) : ""}</span>
          {run.session_id
            ? <button type="button" className="plan-side-panel__action ct-run-row__reveal" aria-label={`View transcript: ${label} run, ${started}`} title="View transcript" onClick={() => onOpen(run)}><MessageSquareText size={14} aria-hidden="true" /></button>
            : <span className="plan-side-panel__action invisible" aria-hidden="true" />}
        </li>;
      })}</ol>}
  </CronCard>;
}
