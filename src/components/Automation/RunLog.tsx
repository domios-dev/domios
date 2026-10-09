import { useId, useRef, useState } from "react";
import { Check, ChevronRight, Copy, Play } from "lucide-react";
import type { BlueprintModel } from "./blueprintModel";
import { formatDuration, nextRunIndex, runTimeline, timelineText, type RunRow, type RunStatus } from "./blueprintRunModel";
import { AUTOMATION_TOOL } from "./styles";

const STATUS_TONE: Record<RunStatus, string> = { running: "bg-[var(--ct-yellow)]", completed: "bg-[var(--ct-green)]", failed: "bg-[var(--ct-red)]", unconfirmed: "bg-[var(--ct-muted)]" };
const ICON_BUTTON = "grid h-7 w-7 shrink-0 place-items-center rounded-md text-[var(--ct-muted-fg)] hover:bg-[var(--ct-bg)] hover:text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--ct-accent)] aria-pressed:text-[var(--ct-accent)]";

export function runTime(at: number): string {
  return new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function Timeline({ row, model }: { row: RunRow; model: BlueprintModel }) {
  const entries = runTimeline(model, row.run);
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard?.writeText(timelineText(row, entries, runTime));
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return <div className="mb-2 ml-[2.1rem] mr-2 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-2.5">
    <ol className="space-y-1.5">
      {entries.map((entry, index) => <li key={index} className="grid grid-cols-[4.5rem_1fr] gap-x-3 text-xs">
        <span className="text-[var(--ct-muted-fg)]">{entry.kind === "decision" ? "Decision" : entry.kind === "source" ? "Source" : entry.kind === "trigger" ? "Trigger" : "Action"}</span>
        <span className="min-w-0 break-words">
          <span className="text-[var(--ct-fg)]">{entry.title}</span>
          {entry.status && <span className="text-[var(--ct-muted-fg)]"> · {entry.status}</span>}
          {entry.detail && <span className="block text-[var(--ct-muted-fg)]">{entry.detail}</span>}
        </span>
      </li>)}
      {entries.length === 0 && <li className="text-xs text-[var(--ct-muted-fg)]">This recorded event has no stage steps to show.</li>}
    </ol>
    {row.run.incomplete && <p className="mt-2 text-xs text-[var(--ct-muted-fg)]">Part of this run is outside the retained history.</p>}
    <button type="button" className={`${AUTOMATION_TOOL} mt-2`} onClick={() => void copy()}>{copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}{copied ? "Copied" : "Copy run"}</button>
  </div>;
}

export function RunLog({ rows, model, selected, truncated = false, height, onReplay }: {
  rows: readonly RunRow[]; model: BlueprintModel; selected: string | null; truncated?: boolean; height?: number;
  onReplay: (row: RunRow) => void;
}) {
  const [expanded, setExpanded] = useState<string | null>(null);
  const [focused, setFocused] = useState(0);
  const list = useRef<HTMLOListElement>(null);
  const headingId = useId();
  const active = Math.min(focused, Math.max(0, rows.length - 1));
  const move = (index: number, code: string) => {
    const next = nextRunIndex(index, rows.length, code);
    if (next === null) return false;
    setFocused(next);
    list.current?.querySelectorAll<HTMLButtonElement>("button[data-run-replay]")[next]?.focus();
    return true;
  };
  return <section id="blueprint-run-log" aria-labelledby={headingId} style={height === undefined ? undefined : { height }} className="flex min-h-0 shrink-0 flex-col bg-[var(--ct-bg)] text-xs">
    <div className="flex items-baseline gap-2 px-4 pb-1.5 pt-3">
      <h3 id={headingId} className="text-[length:var(--ct-font-size-sm)] font-semibold text-[var(--ct-fg)]">Runs</h3>
      {rows.length > 0 && <span className="text-[var(--ct-muted-fg)]">{rows.length} recorded · newest first</span>}
    </div>
    {truncated && <p role="status" className="px-4 pb-1 text-[var(--ct-muted-fg)]">Older runs are outside the retained history.</p>}
    {!rows.length && <p role="status" className="px-4 py-3 text-[var(--ct-muted-fg)]">No runs yet. Each time the trigger fires, a run appears here and its path lights up on the graph.</p>}
    <ol ref={list} aria-label="Blueprint runs" className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
      {rows.map((row, index) => {
        const time = runTime(row.at);
        const isSelected = row.key === selected;
        const open = expanded === row.key;
        const timelineId = `run-steps-${index}`;
        return <li key={row.key}>
          <div className={`ct-row bp-run${isSelected ? " ct-row--selected" : ""}`} data-selected={isSelected || undefined} onClick={(event) => { if (!(event.target as HTMLElement).closest("button")) onReplay(row); }} onKeyDown={(event) => { if (move(index, event.code)) event.preventDefault(); }}>
            <span className={`h-2 w-2 shrink-0 rounded-full ${STATUS_TONE[row.status]}`} title={row.status} aria-hidden="true" />
            <span className="shrink-0 tabular-nums text-[var(--ct-muted-fg)] sm:w-[4.75rem]">{time}</span>
            <span className="bp-chip hidden max-w-[9rem] truncate md:inline" title="Trigger">{row.trigger}</span>
            <span className="min-w-0 flex-1 truncate text-[var(--ct-fg)]" title={row.path.join(" → ")}>{row.path.join(" → ")}</span>
            {row.confidence != null && <span className="bp-chip" data-tone="agent" title="Agent confidence">{Math.round(row.confidence * 100)}%</span>}
            {row.durationMs != null && <span className="hidden shrink-0 tabular-nums text-[var(--ct-muted-fg)] sm:inline">{formatDuration(row.durationMs)}</span>}
            <button type="button" data-run-replay="" tabIndex={index === active ? 0 : -1} className={`bp-run__replay ${ICON_BUTTON}`} aria-pressed={isSelected} aria-label={`Replay run ${time}`} title={`Replay run ${time}`} onFocus={() => setFocused(index)} onClick={() => onReplay(row)}><Play size={14} aria-hidden="true" /></button>
            <button type="button" tabIndex={index === active ? 0 : -1} className={ICON_BUTTON} aria-expanded={open} aria-controls={timelineId} aria-label={`Steps of run ${time}`} title="Show steps" onClick={() => setExpanded(open ? null : row.key)}><ChevronRight size={14} className={`transition-transform motion-reduce:transition-none ${open ? "rotate-90" : ""}`} aria-hidden="true" /></button>
          </div>
          <div id={timelineId} hidden={!open}>{open && <Timeline row={row} model={model} />}</div>
        </li>;
      })}
    </ol>
  </section>;
}
