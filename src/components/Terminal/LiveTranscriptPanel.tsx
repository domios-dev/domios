import { useLayoutEffect, useMemo, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { AudioLines, ChevronDown, Grip, Maximize2, Mic, Minus, RefreshCw, Settings2, X } from "lucide-react";
import { focusTab, askGeneralAgent, workspaceOfTab } from "../../lib/generalAgent";
import type { Tab } from "../../lib/types";
import { useMuxSelector, useMuxStateStore } from "../../store/multiplexer/selectorStore";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { isSidebarTabVisible } from "../Sidebar/workspaceTabs";
import { BusyState } from "../shared/BusyState";
import { createLiveTranscriptApi, directTranscriptAvailable, TRANSCRIPT_PAGE_SIZE, type LiveTranscriptApi, type RoutedRecord, type TranscriptRow, type Trigger } from "../../lib/transport/liveTranscript";
import { webRequestContext } from "../../lib/webApi";
import { useLiveTranscriptCapture } from "./voice/useLiveTranscriptCapture";
import { LiveTranscriptThoughts } from "./LiveTranscriptThoughts";
import { LiveTranscriptContext, useLiveTranscriptAttachments, useLiveTranscriptGeometry, useLiveTranscriptPanel } from "./useLiveTranscriptPanel";
import "./LiveTranscriptPanel.css";

const MODES: Record<Trigger, string> = {
  scout_then_concierge: "Scout then Concierge",
  wake_word: "Wake word",
  direct_batch: "Direct batch",
  manual: "Manual",
};

function usableTab(tab: Tab) {
  return isSidebarTabVisible(tab) && !tab.remote_peer_id && !tab.ownership && !tab.neighbor_authority && !tab.locked;
}

export function LiveTranscriptHost({ children }: { children: ReactNode }) {
  const store = useMuxStateStore();
  const [session, setSession] = useState<{ ownerId: string; identity: number }>();
  const [locked, setLocked] = useState(false);
  const [openCount, setOpenCount] = useState(0);
  const tab = useMuxSelector(state => session ? state.tabs[session.ownerId] : undefined);
  const identity = useMuxSelector(webRequestContext);
  const active = tab && usableTab(tab) && session?.identity === identity && directTranscriptAvailable() ? session : undefined;
  const context = useMemo(() => ({
    ownerId: active?.ownerId ?? null,
    locked: Boolean(active && locked),
    open(tabId: string) {
      if (!directTranscriptAvailable()) return;
      const target = store.getState().tabs[tabId];
      if (!target || !usableTab(target)) return;
      if (!active || active.ownerId !== tabId && !locked) setSession({ ownerId: tabId, identity: webRequestContext() });
      setOpenCount(count => count + 1);
    },
  }), [active, locked, store]);
  return <LiveTranscriptContext.Provider value={context}>
    {children}
    {active && tab && <LiveTranscriptSession key={`${active.identity}:${active.ownerId}`} tab={tab}
      identity={String(active.identity)} openCount={openCount} onLock={setLocked}
      onClose={() => { setLocked(false); setSession(undefined); }} />}
  </LiveTranscriptContext.Provider>;
}

function LiveTranscriptSession({ tab, identity, openCount, onLock, onClose }: {
  tab: Tab; identity: string; openCount: number; onLock: (active: boolean) => void; onClose: () => void;
}) {
  const api = useMemo(() => createLiveTranscriptApi(tab.id), [tab.id]);
  const [autoResume, setAutoResume] = useState(false);
  const capture = useLiveTranscriptCapture({ tabId: tab.id, identity, enabled: true, autoResume });
  const active = capture.phase === "recording" || capture.phase === "starting" || capture.phase === "pausing";
  useLayoutEffect(() => {
    onLock(active);
    return () => onLock(false);
  }, [active, onLock]);
  return createPortal(<LiveTranscriptPanel api={api} owner={tab} openCount={openCount}
    capture={capture} autoResume={autoResume} onAutoResume={setAutoResume} onClose={onClose} />, document.body);
}

type PanelProps = {
  api: LiveTranscriptApi;
  owner: Tab;
  openCount: number;
  capture: ReturnType<typeof useLiveTranscriptCapture>;
  autoResume: boolean;
  onAutoResume: (enabled: boolean) => void;
  onClose: () => void;
};

export function LiveTranscriptPanel({ api, owner, openCount, capture, autoResume, onAutoResume, onClose }: PanelProps) {
  const store = useMuxStateStore();
  const geometry = useLiveTranscriptGeometry();
  const [collapsedAt, setCollapsedAt] = useState<number>();
  const collapsed = collapsedAt === openCount;
  const [log, setLog] = useState(false);
  const [settings, setSettings] = useState(false);
  const [thoughts, setThoughts] = useState<{ epoch: string; rationale?: string }>();
  const [selected, setSelected] = useState<ReadonlySet<number>>(new Set());
  const [feedback, setFeedback] = useState<string>();
  const attachments = useLiveTranscriptAttachments(owner.id, setFeedback);
  const view = useLiveTranscriptPanel({ api, show: !collapsed, log, captureRevision: capture.revision, capturePhase: capture.phase });
  const ownerName = displayTitleForTab(owner);
  const phase = capture.phase;
  const active = phase === "recording" || phase === "starting" || phase === "pausing";
  const status = view.overview?.status;
  const acked = view.overview?.cursor?.ackedSeq;
  const focusOwner = () => focusTab(owner.id, workspaceOfTab(store.getState(), owner.id));
  const page = view.history?.page;
  const canResume = phase === "paused" || (status?.committedSeq ?? 0) > 0;
  const chosen = page?.rows.filter(row => row.content.kind === "speech" && selected.has(row.seq)) ?? [];
  const selectedText = chosen.flatMap(row => row.content.kind === "speech" ? [row.content.text] : []).join("\n\n");
  const ownEpoch = status?.binding.target.kind === "attached" ? status.binding.target.epochId : undefined;
  const scoutUnavailable = status?.scout === "unavailable" || view.readiness?.scout === "unavailable";
  const useGeneral = capture.phase === "setup" || view.readiness?.asr.state === "unavailable" ||
    view.readiness?.asr.state === "unloaded" || view.readiness?.concierge === "unavailable" || scoutUnavailable;
  const routeRange = contiguousSpeechRange(chosen);

  function switchView(toLog: boolean) {
    setLog(toLog);
    setThoughts(undefined);
    setFeedback(undefined);
  }

  function toggleRow(seq: number) {
    setSelected(previous => {
      const next = new Set(chosen.map(row => row.seq));
      if (previous.has(seq)) next.delete(seq);
      else next.add(seq);
      return next;
    });
    setFeedback(undefined);
  }

  function exportSelected() {
    if (!selectedText) return;
    const url = URL.createObjectURL(new Blob([selectedText], { type: "text/plain;charset=utf-8" }));
    const link = document.createElement("a");
    link.href = url;
    link.download = "live-transcript.txt";
    document.body.append(link);
    try {
      link.click();
    } finally {
      link.remove();
      setTimeout(() => URL.revokeObjectURL(url), 60_000);
    }
  }

  if (collapsed) return <aside ref={attachments.container} className="ct-live-orb" aria-label="Collapsed Live Transcript"
    data-drag-over={attachments.webDragOver} style={{ left: geometry.bounds.left, top: geometry.bounds.top }}
    {...attachments.webDropHandlers} onPaste={event => void attachments.handlePasteEvent(event.nativeEvent)}>
    <button type="button" className="ct-live-circle" aria-label="Expand Live Transcript" data-live={active}
      onClick={() => setCollapsedAt(undefined)}><AudioLines size={19} aria-hidden /></button>
    <button type="button" onClick={focusOwner}>{active ? "Live" : "Transcript"} · {ownerName}</button>
  </aside>;

  return <aside ref={attachments.container} className="ct-live-panel" style={geometry.bounds} aria-label={`Live Transcript for ${ownerName}`}
    data-drag-over={attachments.webDragOver} {...attachments.webDropHandlers}
    onPaste={event => void attachments.handlePasteEvent(event.nativeEvent)}>
    <header className="ct-live-header">
      <button type="button" className="ct-live-handle" aria-label="Move Live Transcript" title="Move with the arrow keys or drag"
        {...geometry.handle("move")}><Grip size={16} aria-hidden /></button>
      <button type="button" className="ct-live-owner" onClick={focusOwner} title={`Go to ${ownerName}`}>
        <span className="ct-live-dot" data-live={active} />{active ? "Live" : "Transcript"} · {ownerName}
      </button>
      <div className="ct-live-switch" role="group" aria-label="Transcript view">
        <button type="button" aria-pressed={!log} onClick={() => switchView(false)}>Text</button>
        <button type="button" aria-pressed={log} onClick={() => switchView(true)}>Log</button>
      </div>
      <button type="button" aria-label="Transcript settings" aria-expanded={settings} onClick={() => { setSettings(value => !value); void view.refreshReadiness(); }}>
        <Settings2 size={16} aria-hidden />
      </button>
      <button type="button" aria-label="Collapse Live Transcript" onClick={() => { setThoughts(undefined); setCollapsedAt(openCount); }}><Minus size={16} aria-hidden /></button>
      <button type="button" aria-label="Close Live Transcript" disabled={active} onClick={onClose}><X size={16} aria-hidden /></button>
    </header>
    {(settings || useGeneral || scoutUnavailable || capture.error || view.error) && <div className="ct-live-controls">
      {settings && <RoutingSettings owner={owner.id} view={view} api={api} autoResume={autoResume}
        onAutoResume={onAutoResume} scoutUnavailable={scoutUnavailable} />}
      {useGeneral && <div className="ct-live-callout">
        <button type="button" disabled={view.busy} onClick={() => void view.mutate(() => askGeneralAgent(store.getState(),
          "Set up Live Transcript automatically in a few direct commands (clicking Set up with General is my explicit consent to install the official Transcriber plugin 0.5.1+, the ~487 MB Parakeet TDT 0.6B v3 model, and its local Python runtime — do not pause to ask or send me to Extensions). Run directly without exploratory checks:\n" +
          "1. Ensure Transcriber >= 0.5.1: `domios plugin update transcriber` (or `domios plugin install transcriber` if missing/local).\n" +
          "2. Check `domios plugin transcriber dictation-status`. If not ready, run `domios plugin transcriber dictation-install parakeet-tdt-0.6b-v3-int8 --consent`, track `domios plugin transcriber dictation-progress` until complete, then run `domios plugin transcriber dictation-select parakeet-tdt-0.6b-v3-int8` and `domios plugin transcriber dictation-test parakeet-tdt-0.6b-v3-int8`.\n" +
          "3. Reply with one short confirmation once ready (or the exact error if a step failed). Keep existing voice-note settings and drafts untouched."))}>Set up with General</button>
      </div>}
      {scoutUnavailable && <div className="ct-live-callout" role="status">
        Scout is unavailable. Choose Wake word, Direct batch, or Manual to route speech.
        {!settings && <button type="button" onClick={() => setSettings(true)}>Choose routing mode</button>}
      </div>}
      {(capture.error || view.error) && <p role="alert" className="ct-live-callout">{capture.error || view.error}</p>}
    </div>}
    <div ref={view.scrollRef} className="ct-live-document" onScroll={view.onScroll} tabIndex={0}
      aria-label={log ? "Routing timeline" : "Transcript text"}>
      {!view.loaded && !view.error && <BusyState inline label="Reading transcript…" phase="work" />}
      {log ? <>
        {view.events?.page.actions.length === 0 && <p className="ct-live-muted">No routed actions on this page.</p>}
        {view.events?.page.actions.map(record => <RouteCard key={record.action.actionId} record={record} />)}
      </> : <>
        {page?.gaps.length ? <p className="ct-live-muted">Some earlier speech has been compacted.</p> : null}
        {page?.rows.length === 0 && !capture.provisional && <p className="ct-live-muted">Committed speech will appear here.</p>}
        {page?.rows.map((row, index) => <div key={row.seq}>
          {acked != null && row.seq > acked && (index === 0 || page.rows[index - 1].seq <= acked) &&
            <div className="ct-live-marker">Acknowledged through {acked}</div>}
          {row.content.kind === "speech" ? <label className="ct-live-speech">
            <input type="checkbox" checked={selected.has(row.seq)} onChange={() => toggleRow(row.seq)}
              aria-label={`Select speech at ${clock(row.atMs)} (segment ${row.seq})`} />
            <span>{row.content.text}</span>
          </label> : <div className="ct-live-marker">
            {row.content.kind === "paused" ? "Capture paused" : "Capture resumed"} · {clock(row.atMs)}
          </div>}
        </div>)}
        {acked != null && page?.rows.length && acked >= page.rows[page.rows.length - 1].seq ?
          <div className="ct-live-marker">Acknowledged through {acked}</div> : null}
        {capture.provisional && page?.nextFromSeq == null &&
          <p className="ct-live-draft"><span className="sr-only">Live draft </span>{capture.provisional}</p>}
      </>}
    </div>
    {log && <div className="ct-live-toolbar ct-live-paging">
      <button type="button" disabled={!view.events?.after} onClick={() => {
        setThoughts(undefined);
        view.moveLog(Math.max(0, (view.events?.after ?? 0) - TRANSCRIPT_PAGE_SIZE));
      }}>Previous actions</button>
      <button type="button" disabled={view.events?.page.nextAfterSeq == null}
        onClick={() => { if (view.events?.page.nextAfterSeq != null) { setThoughts(undefined); view.moveLog(view.events.page.nextAfterSeq); } }}>Next actions</button>
      <LogEpochPicker key={view.events?.after ?? 0} current={ownEpoch} records={view.events?.page.actions ?? []}
        open={thoughts} onOpen={setThoughts} />
    </div>}
    {log && thoughts && <LiveTranscriptThoughts key={thoughts.epoch} api={api} epoch={thoughts.epoch}
      rationale={thoughts.rationale} onClose={() => setThoughts(undefined)} />}
    {!log && <div className="ct-live-toolbar ct-live-paging">
      <button type="button" disabled={(view.history?.from ?? 1) <= 1}
        onClick={() => { setSelected(new Set()); view.moveHistory(Math.max(1, (view.history?.from ?? 1) - TRANSCRIPT_PAGE_SIZE)); }}>Earlier speech</button>
      <button type="button" disabled={page?.nextFromSeq == null} onClick={() => {
        if (page?.nextFromSeq != null) { setSelected(new Set()); view.moveHistory(page.nextFromSeq); }
      }}>Later speech</button>
      <button type="button" onClick={() => { setSelected(new Set()); view.moveHistory(null); }}>Latest speech</button>
    </div>}
    {!log && selectedText && <div className="ct-live-toolbar ct-live-selection">
      <span>{chosen.length} selected</span>
      <button type="button" onClick={exportSelected}>Export</button>
      <button type="button" disabled={view.busy} onClick={async () => {
        if (await view.mutate(signal => api.save("Live Transcript selection", selectedText, signal))) setFeedback("Selection saved to Memory.");
      }}>Save to Memory</button>
      {status?.trigger === "manual" && <button type="button" disabled={view.busy || !routeRange}
        onClick={() => { if (routeRange) void view.mutate(signal => api.route(routeRange, signal)); }}>Route selection</button>}
    </div>}
    <footer className="ct-live-footer">
      <span role="status"><span className="ct-live-dot" data-live={active} />{feedback ?? captureLabel(phase)}</span>
      <button type="button" aria-label="Refresh transcript" title="Refresh transcript" onClick={() => { void view.refresh(); void view.refreshReadiness(); }}>
        <RefreshCw size={14} aria-hidden />
      </button>
      {active ? <button type="button" disabled={phase === "pausing"} onClick={() => void capture.pause()}>Pause</button> :
        <button type="button" onClick={() => void capture.start(canResume)}>
          <Mic size={14} aria-hidden />{canResume ? "Resume" : "Start"}
        </button>}
      <button type="button" className="ct-live-resize ct-live-handle" aria-label="Resize Live Transcript" title="Resize with the arrow keys or drag"
        {...geometry.handle("size")}><Maximize2 size={15} aria-hidden /></button>
    </footer>
  </aside>;
}

function captureLabel(phase: PanelProps["capture"]["phase"]): string {
  switch (phase) {
    case "recording": return "Microphone active";
    case "starting": return "Opening microphone";
    case "pausing": return "Flushing speech";
    case "paused": return "Paused";
    default: return "Microphone off";
  }
}

function clock(milliseconds: number) {
  return new Date(milliseconds).toLocaleTimeString([], { hour: "numeric", minute: "2-digit", second: "2-digit" });
}

function contiguousSpeechRange(rows: TranscriptRow[]) {
  const first = rows[0]?.seq;
  if (first === undefined || rows.some((row, index) => row.seq !== first + index)) return null;
  return { fromSeq: first, toSeq: first + rows.length - 1 };
}

function RouteCard({ record }: { record: RoutedRecord }) {
  const { action, binding } = record;
  const { fromSeq, toSeq } = action.range;
  const target = useMuxSelector(state => binding.target.kind === "external" ? state.tabs[binding.target.tabId] : undefined);
  const destination = binding.target.kind === "attached" ? "Transcript Concierge"
    : target && isSidebarTabVisible(target) ? displayTitleForTab(target) : binding.target.kind === "external" ? "External tab" : action.destination;
  return <article className="ct-live-route" aria-label={`Routed speech to ${destination}`}>
    <div><strong>{fromSeq === toSeq ? `Speech ${fromSeq}` : `Speech ${fromSeq}–${toSeq}`}</strong>
      <time dateTime={new Date(action.atMs).toISOString()}>{clock(action.atMs)}</time></div>
    <p>{destination}</p>
    <p className="ct-live-rationale">{action.rationale}</p>
  </article>;
}

function LogEpochPicker({ current, records, open, onOpen }: {
  current?: string;
  records: RoutedRecord[];
  open?: { epoch: string; rationale?: string };
  onOpen: (value: { epoch: string; rationale?: string } | undefined) => void;
}) {
  const [receipt, setReceipt] = useState("");
  const record = records.find(item => item.action.actionId === receipt);
  const epoch = record ? record.binding.target.kind === "attached" ? record.binding.target.epochId : undefined : current;
  return <>
    <select aria-label="Thoughts for" value={record ? receipt : ""} onChange={event => {
      setReceipt(event.target.value);
      onOpen(undefined);
    }}>
      <option value="">Current epoch</option>
      {records.map(item => <option key={item.action.actionId} value={item.action.actionId}>
        {clock(item.action.atMs)} · Speech {item.action.range.fromSeq}{item.binding.target.kind !== "attached" ? " · External" : ""}
      </option>)}
    </select>
    <button type="button" aria-expanded={Boolean(open)} disabled={!epoch}
      onClick={() => onOpen(open ? undefined : epoch ? { epoch, rationale: record?.action.rationale } : undefined)}>
      Thoughts <ChevronDown size={13} aria-hidden />
    </button>
    {!epoch && <span className="ct-live-muted">Reasoning unavailable</span>}
  </>;
}

function RoutingSettings({ owner, view, api, autoResume, onAutoResume, scoutUnavailable }: {
  owner: string;
  view: ReturnType<typeof useLiveTranscriptPanel>;
  api: LiveTranscriptApi;
  autoResume: boolean;
  onAutoResume: (value: boolean) => void;
  scoutUnavailable: boolean;
}) {
  const tabs = useMuxSelector(state => state.tabs);
  const status = view.overview?.status;
  const target = status?.binding.target;
  const configuredTab = target?.kind === "external" ? target.tabId : "";
  const [choice, setChoice] = useState<{ trigger?: Trigger; externalTab?: string }>({});
  const trigger = choice.trigger ?? status?.trigger ?? "scout_then_concierge";
  const externalTab = choice.externalTab ?? configuredTab;
  const externalTabs = Object.values(tabs).filter(tab => tab.id !== owner && usableTab(tab) && tab.agent_type);
  const canApply = status && (target?.kind === "detached" || externalTab !== configuredTab || trigger !== status.trigger) &&
    !(scoutUnavailable && trigger === "scout_then_concierge") && (!externalTab || externalTabs.some(tab => tab.id === externalTab));
  return <div className="ct-live-settings">
    <label>Routing mode
      <select value={trigger} onChange={event => setChoice(value => ({ ...value, trigger: event.target.value as Trigger }))}>
        {Object.entries(MODES).map(([id, title]) => <option value={id} key={id}
          disabled={id === "scout_then_concierge" && scoutUnavailable}>{title}</option>)}
      </select>
    </label>
    <label>Route to
      <select value={externalTab} onChange={event => setChoice(value => ({ ...value, externalTab: event.target.value }))}>
        <option value="">Transcript Concierge</option>
        {externalTab && !externalTabs.some(tab => tab.id === externalTab) && <option value={externalTab}>Unavailable external tab</option>}
        {externalTabs.map(tab => <option key={tab.id} value={tab.id}>{displayTitleForTab(tab)}</option>)}
      </select>
    </label>
    {!externalTab && <div className="ct-live-muted">Provider {status?.router?.provider ?? "daemon default"} · Model {status?.router?.model ?? "daemon default"} · Turn cap {status?.router?.cap ?? "daemon default"}</div>}
    <label className="ct-live-autoresume"><input type="checkbox" checked={autoResume} onChange={event => onAutoResume(event.target.checked)} />
      Pause when this window loses focus and resume on return
    </label>
    <button type="button" disabled={!canApply || view.busy} onClick={() => {
      void view.mutate(signal => api.attach(target?.kind !== "detached" && externalTab === configuredTab
        ? { modeOnly: true, trigger } : { trigger, ...(externalTab ? { externalTab } : {}) }, signal));
    }}>Apply routing</button>
  </div>;
}
