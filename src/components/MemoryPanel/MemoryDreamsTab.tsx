import { memo, useCallback, useState } from "react";
import { Play } from "lucide-react";
import type { ApiConfig, GraphWorkerInfo } from "../../lib/types";
import { formatUntil } from "../../lib/timeLabels";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { Loader } from "../shared/Loader";
import { AgentRunLogPopup } from "../AgentRun/AgentRunLogPopup";
import { RunChatPopup } from "../AgentRun/RunChatPopup";
import { defaultSpawnProviderId } from "../../lib/agentDescriptorCache";
import { isTerminalWorkerStatus } from "../AgentRun/workerEmptyState";
import { DreamCard } from "./DreamCard";
import { DreamDetail, DreamLive } from "./DreamDetail";
import { DreamOverview } from "./DreamOverview";
import { dreamNights, selectDreamNight } from "./dreamModel";
import { useDreams } from "./useDreams";
import { useDreamLayout, useDreamSheet } from "./useDreamDetail";
import "./dreams.css";

interface Props {
  api: ApiConfig;
  onViewInGraph?: (id: string) => void;
}

function MemoryDreamsTabImpl({ api, onViewInGraph }: Props) {
  const data = useDreams(api);
  const { ref, compact } = useDreamLayout();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detailDismissed, setDetailDismissed] = useState(false);
  const [sheetOpen, setSheetOpen] = useState(false);
  const [logWorker, setLogWorker] = useState<GraphWorkerInfo | null>(null);
  const closeSheet = useCallback(() => setSheetOpen(false), []);
  const sheet = useDreamSheet(compact && sheetOpen);
  const nights = dreamNights(data.diaryEntries, data.dreamWorkers);
  const live = nights.find((night) => night.state === "live");
  const selected = detailDismissed ? undefined : selectDreamNight(nights.filter((night) => night.state !== "live"), selectedId);
  const detail = selected && <DreamDetail key={selected.id} night={selected} api={api} onClose={() => { setDetailDismissed(true); closeSheet(); }}
    onLog={() => selected.worker && setLogWorker(selected.worker)} onRun={data.runNow} onViewInGraph={onViewInGraph} />;
  const cron = data.dreamCron;
  const next = cron?.next_run ? new Date(cron.next_run).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", timeZone: cron.tz ?? "UTC" }) : null;
  return <div ref={ref} className="dreams-root" data-testid="memory-dreams-tab">
    <div className="dreams-page">
      <div className="dream-row dream-title">
        <h2>Dream diary</h2>
        <button className="dream-schedule dream-small dream-muted" type="button"
          onClick={() => cron && data.toggleDreamCron(!cron.enabled)} disabled={!cron}
          aria-label={cron?.enabled ? "Disable nightly dreaming" : "Enable nightly dreaming"}>
          <span className={["dream-dot", cron?.enabled ? "" : "dream-inactive"].join(" ")} />
          {cron ? cron.enabled ? <>Nightly{next && <> at {next}</>}{cron.next_run && <> · next {formatUntil(cron.next_run, Date.now())}</>}</> : "Nightly off" : "Not configured"}
        </button>
        <span className="dream-spacer" />
        {!live && <button className="dream-button" data-testid="memory-dream-run-now" onClick={data.runNow} disabled={data.running}>
          <Play size={11} aria-hidden="true" />{data.running ? "Starting…" : "Dream now"}
        </button>}
      </div>
      {data.runError && <InlineErrorNotice error={data.runError} operation="running memory maintenance" blocking />}
      {data.diaryLoading ? <Loader variant="region" label="Loading dream diary" /> : <div className="dream-columns">
        <div className="dream-nights">
          {compact && live ? <DreamLive night={live} onStop={() => data.stop(live.worker!)} /> : <DreamOverview nights={nights} now={Date.now()} />}
          {nights.filter((night) => night.state !== "live").map((night) => <DreamCard key={night.id} night={night}
            selected={!compact && !(live && !selectedId) && selected?.id === night.id} onOpen={() => { setSelectedId(night.id); setDetailDismissed(false); if (compact) setSheetOpen(true); }} />)}
          {!nights.length && <div className="dream-card dream-empty" data-testid="memory-dream-empty"><b>No dreams yet</b><p className="dream-muted">Start a session or wait for the nightly run.</p></div>}
        </div>
        {!compact && <aside>{live && !selectedId ? <DreamLive night={live} onStop={() => data.stop(live.worker!)} /> : selected && <div className="dream-card dream-panel">{detail}</div>}</aside>}
      </div>}
    </div>
    {compact && <dialog ref={sheet} className="dream-sheet" onClose={closeSheet} aria-label="Night details" onClick={(event) => { if (event.target === event.currentTarget) closeSheet(); }}>
      <div className="dream-grab" />{sheetOpen && detail}
    </dialog>}
    {logWorker && (logWorker.session_id ? <RunChatPopup tabId={logWorker.tab_id}
      title={logWorker.intent.replace(/^dreaming:/, "dream:")} subtitle={[logWorker.agent_provider, logWorker.model].filter(Boolean).join(" / ")}
      status={logWorker.status} report={logWorker.report} provider={logWorker.agent_provider ?? defaultSpawnProviderId()}
      cwd={logWorker.cwd ?? ""} sessionId={logWorker.session_id} live={!isTerminalWorkerStatus(logWorker.status)} onClose={() => setLogWorker(null)} />
      : <AgentRunLogPopup worker={logWorker} api={api} onClose={() => setLogWorker(null)} />)}
  </div>;
}

export const MemoryDreamsTab = memo(MemoryDreamsTabImpl);
