import type { CSSProperties } from "react";
import { History } from "lucide-react";
import { formatTime } from "../../../lib/formatters";
import type { CronRunInfo } from "../../../lib/types";
import { CronCard, formatRunDuration, missedOccurrence, runOutcomeLabel, runTone, Unavailable } from "./parts";

const historyTime = new Intl.DateTimeFormat(undefined, {
  month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit",
});

function RunTime({ label, at }: { label: string; at: number }) {
  const iso = new Date(at).toISOString();
  return <time className="block" dateTime={iso} title={iso}>{label} {historyTime.format(at)}</time>;
}

export function CronRunHistory({ runs, allowed, reason, onOpen, runLabel }: { runs: readonly CronRunInfo[]; allowed: boolean; reason?: string; onOpen: (run: CronRunInfo) => void; runLabel?: string }) {
  const listed = allowed && runs.length > 0;
  return <CronCard label="Run history" icon={<History size={14} />} bodyless={listed} aside={listed ? <span className="ct-card__meta shrink-0">{runs.length} recent</span> : undefined}>
    {!allowed ? <Unavailable operation="history" reason={reason} />
      : runs.length === 0 ? <p className="ct-job-note">No runs yet</p>
      : <ol aria-label="Run history" className="ct-rows">{runs.map((run) => {
        const started = formatTime(run.started_at);
        const missed = missedOccurrence(run);
        const outcome = runLabel && run.summary && !missed ? run.summary : runOutcomeLabel(run);
        const late = run.late_by_ms == null ? "" : ` · Late by ${formatRunDuration(run.late_by_ms)}`;
        const label = `${runLabel ? `${runLabel} · ` : ""}${outcome}${late}`;
        const live = run.status === "Running" || run.status === "AwaitingOwnerPrompt";
        const content = <>
          <span className="ct-detail__dot" data-live={live || undefined} style={{ "--ct-detail-tone": runTone(run.status) } as CSSProperties} aria-hidden="true" />
          <span className="ct-run-row__main py-1">
            <span className="block">{label}</span>
            <span className="ct-run-row__time block">
              {run.scheduled_occurrence_at != null && <RunTime label="Scheduled" at={run.scheduled_occurrence_at} />}
              {run.fired_at == null
                ? <RunTime label={missed ? "Recorded" : "Started"} at={run.started_at} />
                : <>
                    <RunTime label={run.fired_at === run.started_at ? "Fired and admitted" : "Fired"} at={run.fired_at} />
                    {run.fired_at !== run.started_at && <RunTime label="Admitted" at={run.started_at} />}
                  </>}
            </span>
          </span>
          <span className="ct-run-row__meta">{!missed && run.finished_at ? formatRunDuration(run.finished_at - run.started_at) : ""}</span>
        </>;
        return <li key={run.run_id} data-run-status={run.status}>
          {run.session_id
            ? <button type="button" className="ct-row ct-run-row" aria-label={`View transcript: ${label} run, ${started}`} title="View transcript" onClick={() => onOpen(run)}>{content}</button>
            : <div className="ct-row ct-run-row">{content}</div>}
        </li>;
      })}</ol>}
  </CronCard>;
}
