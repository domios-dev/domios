import { useState } from "react";
import { X } from "lucide-react";
import type { ApiConfig } from "../../lib/types";
import { DreamChips, DreamDate, dreamInsight } from "./DreamCard";
import { DreamConstellation } from "./DreamConstellation";
import { DREAM_STAGES, dreamRunData, type DreamNight } from "./dreamModel";
import { useDreamLabels } from "./useDreamDetail";

const ACTION_LABELS = { linked: "Linked", merged: "Merged", archived: "Archived", reweighted: "Re-weighted" };

export function DreamDetail({ night, api, onClose, onLog, onRun, onViewInGraph }: {
  night: DreamNight; api: ApiConfig; onClose?: () => void; onLog: () => void; onRun: () => void; onViewInGraph?: (id: string) => void;
}) {
  const data = dreamRunData(night.worker);
  const titles = useDreamLabels(api, data.actions.map(([id]) => id));
  const [copied, setCopied] = useState(false);
  const insight = dreamInsight(night);
  const lead = insight.match(/^.*?[.!?]+(?=\s|$)/su)?.[0] ?? insight;
  const rest = insight.slice(lead.length).trim();
  const c = night.counters;
  const hasMerged = data.actions.some(([, action]) => action === "merged");
  const hasArchived = data.actions.some(([, action]) => action === "archived");
  return <section className="dream-detail" aria-label="Selected night" data-testid="memory-dream-detail" data-dream-id={night.id}>
    <div className="dream-row"><DreamDate night={night} /><span className="dream-spacer" />
      {onClose && <button className="dream-close" onClick={onClose} aria-label="Close night details"><X size={13} /></button>}
    </div>
    {night.state === "stopped" ? <>
      <div className="dream-row dream-learned"><span className="dream-dot dream-red" />This night stopped.</div>
      <p className="dream-muted">{insight}</p>
      <dl className="dream-kv">
        <div><dt>Status</dt><dd>{night.worker?.status}</dd></div>
        {night.worker?.model && <div><dt>Model</dt><dd>{night.worker.model}</dd></div>}
        <div><dt>Diary</dt><dd>{night.entry ? "Saved before stopping" : "No diary written"}</dd></div>
      </dl>
      <div className="dream-row dream-small"><button className="dream-link" onClick={onLog}>Open run log</button><span className="dream-muted">·</span><button className="dream-link" onClick={onRun}>Dream now</button></div>
    </> : <>
      {data.hasConstellation && <>
        <div className="dream-before-after"><DreamConstellation worker={night.worker} seed={night.id} before /><span className="dream-muted">→</span><DreamConstellation worker={night.worker} seed={night.id} /></div>
        <div className="dream-row dream-legend dream-muted dream-small"><span><i className="dream-line" />new link</span>{hasArchived && <span><i className="dream-ring" />archived</span>}{hasMerged && <span><i className="dream-dot" />merged</span>}</div>
        {data.ids.length > 24 && <span className="dream-small dream-muted">+{data.ids.length - 24} more</span>}
      </>}
      <div><div className="dream-cap">What memory learned</div><div className="dream-learned">{lead}</div>{rest && <p className="dream-muted dream-clamp3 dream-rest">{rest}</p>}</div>
      {night.state === "incomplete" && <div className="dream-small dream-muted">Incomplete diary · this run has no verified final diary.</div>}
      <DreamChips counters={c} />
      {data.hasItems ? <dl className="dream-kv">{data.actions.map(([id, action]) => <div key={id}><dt>{ACTION_LABELS[action]}</dt>
        <dd title={titles[id] ?? id} className={!titles[id] ? "dream-id dream-muted" : undefined}>{titles[id] ?? id}</dd></div>)}</dl>
        : <dl className="dream-kv" data-testid="memory-dream-counters">
          <div><dt>Linked</dt><dd>{c.links} islands and orphans</dd></div>
          <div><dt>Merged</dt><dd>{c.merged} duplicates</dd></div>
          <div><dt>Re-weighted</dt><dd>{c.reweighted} memories</dd></div>
          <div><dt>Tidied</dt><dd>{c.collapsed} duplicate edges, {c.backlinks} diary backlinks</dd></div>
          <div><dt>Archived</dt><dd>{c.archived || "none"}</dd></div>
        </dl>}
      <div className="dream-row dream-small">
        {onViewInGraph && data.ids[0] && <><button className="dream-link" onClick={() => onViewInGraph(data.ids[0])}>Open in graph</button><span className="dream-muted">·</span></>}
        {night.entry && <button className="dream-link" onPointerLeave={() => setCopied(false)} onBlur={() => setCopied(false)} onClick={async () => {
          try { await navigator.clipboard.writeText(night.entry!.body); setCopied(true); } catch { setCopied(false); }
        }}>{copied ? "Copied" : "Copy diary"}</button>}
        {night.worker && <button className="dream-link" onClick={onLog}>Open run log</button>}
      </div>
    </>}
  </section>;
}

export function DreamLive({ night, onStop }: { night: DreamNight; onStop: () => void }) {
  const worker = night.worker!;
  const current = DREAM_STAGES.findIndex(([id]) => id === worker.stage);
  return <section className="dream-card dream-live" aria-label="Dreaming now">
    <div className="dream-row"><span className="dream-dot dream-live-dot" /><b>Dreaming now</b><span className="dream-small dream-muted">started {new Date(night.at).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" })}</span><span className="dream-spacer" /><button className="dream-link dream-muted" onClick={onStop}>Stop</button></div>
    <DreamConstellation worker={worker} seed={night.id} />
    <div className="dream-bar" role="progressbar" aria-label={worker.progress_text ?? "Dreaming"}><i /></div>
    {current >= 0 ? <ol className="dream-steps">{DREAM_STAGES.map(([id, label], index) => <li key={id} className={index < current ? "dream-step-done" : index === current ? "dream-step-now" : ""} aria-current={index === current ? "step" : undefined}>
      <i>{index < current ? "✓" : ""}</i>{label}{index === current && worker.progress_text && <span className="dream-muted"> · {worker.progress_text}</span>}
    </li>)}</ol> : worker.progress_text && <p className="dream-small dream-muted">{worker.progress_text}</p>}
    <div className="dream-small dream-muted">You can keep working; nothing is locked.</div>
  </section>;
}
