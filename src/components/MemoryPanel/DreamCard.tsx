import { Archive, GitMerge, Link, Scale, Brush } from "lucide-react";
import type { CSSProperties } from "react";
import { formatDuration } from "../../lib/timeLabels";
import { parseDream } from "./dreamVisuals";
import { type DreamCounters, type DreamNight } from "./dreamModel";
import { DreamConstellation } from "./DreamConstellation";
export type { DreamDiaryEntry } from "./dreamModel";

export function DreamDate({ night }: { night: DreamNight }) {
  const date = new Date(night.at);
  return <div className="dream-row dream-small">
    <b>{date.toLocaleDateString("en-US", { weekday: "long" })}</b>
    <time dateTime={date.toISOString()} className="dream-muted">
      {date.toLocaleDateString("en-US", { month: "short", day: "numeric" })} · {date.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" })}
      {night.worker?.done_at != null && <> · {formatDuration(night.worker.done_at - night.worker.spawned_at)}</>}
    </time>
  </div>;
}

export function DreamChips({ counters: c }: { counters: DreamCounters }) {
  const chips = [
    { n: c.links, label: "links", Icon: Link, tone: "var(--ct-accent)", prefix: "+" },
    { n: c.merged, label: "merged", Icon: GitMerge, tone: "var(--dream-moon)" },
    { n: c.archived, label: "archived", Icon: Archive },
    { n: c.reweighted, label: "re-weighted", Icon: Scale },
    { n: c.collapsed + c.backlinks, label: "edges tidied", Icon: Brush },
  ];
  return <div className="dream-row dream-chips">{chips.filter((chip) => chip.n > 0).map(({ n, label, Icon, tone, prefix }) =>
    <span className="dream-chip" key={label} style={{ "--dream-tone": tone } as CSSProperties}>
      <Icon size={12} aria-hidden="true" /><b>{prefix}{n.toLocaleString("en-US")}</b>{label}
    </span>)}
  </div>;
}

export function dreamInsight(night: DreamNight) {
  if (night.state === "stopped") return (night.worker?.finalization && "reason" in night.worker.finalization ? night.worker.finalization.reason : null) ?? night.worker?.report ?? "This night stopped before a diary was written.";
  if (night.state === "incomplete") return night.entry && parseDream(night.entry.body).insight || "No insight recorded for this run.";
  return night.entry ? parseDream(night.entry.body).insight : night.worker?.progress_text ?? "Dreaming now";
}

export function DreamCard({ night, selected, onOpen }: { night: DreamNight; selected: boolean; onOpen: () => void }) {
  const hasThumb = night.state === "stopped" || !!night.worker?.touched_nodes.length;
  return <button type="button" className={["dream-card dream-night", selected ? "dream-chosen" : "", hasThumb ? "dream-with-thumb" : ""].join(" ")}
    aria-pressed={selected} onClick={onOpen} data-testid="memory-dream-card" data-dream-id={night.id}>
    {night.state === "stopped" ? <div className="dream-stopped"><span className="dream-dot dream-red" />Stopped</div>
      : <DreamConstellation worker={night.worker} seed={night.id} thumbnail />}
    <div className="dream-night-text"><DreamDate night={night} />
      <div className={["dream-learned dream-clamp2", night.state === "stopped" ? "dream-muted" : ""].join(" ")}>{dreamInsight(night)}</div>
      {night.state === "stopped" ? <div className="dream-small dream-muted">{night.entry ? "Diary incomplete" : "No diary written"} · Details</div>
        : <><DreamChips counters={night.counters} />{night.state === "incomplete" && <span className="dream-small dream-muted">Incomplete</span>}</>}
    </div>
  </button>;
}
