import type { CSSProperties } from "react";
import { History } from "lucide-react";
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
        const content = <>
          <span className="ct-detail__dot" data-live={live || undefined} style={{ "--ct-detail-tone": runTone(run.status) } as CSSProperties} aria-hidden="true" />
          <span className="ct-run-row__time">{started}</span>
          <span className="ct-run-row__main">{label}</span>
          <span className="ct-run-row__meta">{run.finished_at ? formatRunDuration(run.finished_at - run.started_at) : ""}</span>
        </>;
        return <li key={run.run_id} data-run-status={run.status}>
          {run.session_id
            ? <button type="button" className="ct-row ct-run-row" aria-label={`View transcript: ${label} run, ${started}`} title="View transcript" onClick={() => onOpen(run)}>{content}</button>
            : <div className="ct-row ct-run-row">{content}</div>}
        </li>;
      })}</ol>}
  </CronCard>;
}
