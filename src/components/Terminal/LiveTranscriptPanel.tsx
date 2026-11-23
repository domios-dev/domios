import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { AudioLines, ChevronDown, ChevronUp, ChevronsDown, Download, GripVertical, Mic, Minus, Pause, RefreshCw, Settings2, X } from "lucide-react";
import { DropdownSelect, type DropdownSelectOption } from "../shared/DropdownMenu";
import { LiquidToggle } from "../shared/LiquidToggle";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { focusTab, askGeneralAgent, generalAgentTab, workspaceOfTab } from "../../lib/generalAgent";
import type { Tab } from "../../lib/types";
import { useMuxSelector, useMuxStateStore } from "../../store/multiplexer/selectorStore";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { isSidebarTabVisible } from "../Sidebar/workspaceTabs";
import { BusyState } from "../shared/BusyState";
import { createLiveTranscriptApi, directTranscriptAvailable, TRANSCRIPT_PAGE_SIZE, type LiveTranscriptApi, type RoutedRecord, type RouterActivity, type TranscriptOverview, type TranscriptReadiness, type TranscriptRow, type Trigger } from "../../lib/transport/liveTranscript";
import { webRequestContext } from "../../lib/webApi";
import { useLiveTranscriptCapture } from "./voice/useLiveTranscriptCapture";
import { LiveTranscriptThoughts } from "./LiveTranscriptThoughts";
import { LiveTranscriptContext, useLiveTranscriptAttachments, useLiveTranscriptGeometry, useLiveTranscriptPanel, type LiveTranscriptOpenOptions } from "./useLiveTranscriptPanel";
import "./LiveTranscriptPanel.css";

const MODES: Record<Trigger, string> = {
  scout_then_concierge: "Scout then Concierge",
  wake_word: "Wake word",
  direct_batch: "Direct batch",
  manual: "Manual",
};

const VIEW_OPTIONS: SegmentedOption<"text" | "log">[] = [{ value: "text", label: "Text" }, { value: "log", label: "Log" }];

function modeOptions(scoutUnavailable: boolean): DropdownSelectOption[] {
  return (Object.keys(MODES) as Trigger[]).map(id => ({
    value: id,
    label: MODES[id],
    description: id === "scout_then_concierge" && scoutUnavailable ? "Scout is unavailable" : undefined,
    disabled: id === "scout_then_concierge" && scoutUnavailable,
  }));
}

function usableTab(tab: Tab) {
  return isSidebarTabVisible(tab) && !tab.remote_peer_id && !tab.ownership && !tab.neighbor_authority && !tab.locked;
}

export function LiveTranscriptHost({ children }: { children: ReactNode }) {
  const store = useMuxStateStore();
  const [session, setSession] = useState<{ ownerId: string; identity: number }>();
  const [locked, setLocked] = useState(false);
  const [{ openCount, autoStartCount }, setOpening] = useState({ openCount: 0, autoStartCount: 0 });
  const tab = useMuxSelector(state => session ? state.tabs[session.ownerId] : undefined);
  const identity = useMuxSelector(webRequestContext);
  const active = tab && usableTab(tab) && session?.identity === identity && directTranscriptAvailable() ? session : undefined;
  const context = useMemo(() => {
    function canOpen(tabId: string) {
      const target = store.getState().tabs[tabId];
      return Boolean(target && usableTab(target) && directTranscriptAvailable());
    }
    return {
      ownerId: active?.ownerId ?? null,
      locked: Boolean(active && locked),
      canOpen,
      open(tabId: string, options?: LiveTranscriptOpenOptions) {
        if (!canOpen(tabId)) return;
        if (!active || active.ownerId !== tabId && !locked) setSession({ ownerId: tabId, identity: webRequestContext() });
        setOpening(({ openCount }) => {
          const next = openCount + 1;
          return { openCount: next, autoStartCount: options?.autostart ? next : 0 };
        });
      },
    };
  }, [active, locked, store]);
  return <LiveTranscriptContext.Provider value={context}>
    {children}
    {active && tab && <LiveTranscriptSession key={`${active.identity}:${active.ownerId}`} tab={tab}
      identity={String(active.identity)} openCount={openCount} autoStartCount={autoStartCount} onLock={setLocked}
      onClose={() => { setLocked(false); setSession(undefined); }} />}
  </LiveTranscriptContext.Provider>;
}

function LiveTranscriptSession({ tab, identity, openCount, autoStartCount, onLock, onClose }: {
  tab: Tab; identity: string; openCount: number; autoStartCount: number; onLock: (active: boolean) => void; onClose: () => void;
}) {
  const api = useMemo(() => createLiveTranscriptApi(tab.id), [tab.id]);
  const [autoResume, setAutoResume] = useState(false);
  const capture = useLiveTranscriptCapture({ tabId: tab.id, identity, enabled: true, autoResume });
  const active = capture.phase === "recording" || capture.phase === "starting" || capture.phase === "pausing";
  useLayoutEffect(() => {
    onLock(active);
    return () => onLock(false);
  }, [active, onLock]);
  return createPortal(<LiveTranscriptPanel api={api} owner={tab} openCount={openCount} autoStartCount={autoStartCount}
    capture={capture} autoResume={autoResume} onAutoResume={setAutoResume} onClose={onClose} />, document.body);
}

type PanelProps = {
  api: LiveTranscriptApi;
  owner: Tab;
  openCount: number;
  autoStartCount?: number;
  capture: ReturnType<typeof useLiveTranscriptCapture>;
  autoResume: boolean;
  onAutoResume: (enabled: boolean) => void;
  onClose: () => void;
};

export function LiveTranscriptPanel({ api, owner, openCount, autoStartCount = 0, capture, autoResume, onAutoResume, onClose }: PanelProps) {
  const store = useMuxStateStore();
  const geometry = useLiveTranscriptGeometry();
  const [collapsedAt, setCollapsedAt] = useState<number>();
  const collapsed = collapsedAt === openCount;
  const [log, setLog] = useState(false);
  const [settings, setSettings] = useState(false);
  const [thoughts, setThoughts] = useState<{ epoch: string; rationale?: string }>();
  const [selected, setSelected] = useState<ReadonlySet<number>>(new Set());
  const [feedback, setFeedback] = useState<string>();
  const [pendingAutoStart, setPendingAutoStart] = useState(false);
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
  const scoutUnavailable = (view.readiness?.scout ?? status?.scout) === "unavailable";
  const isConfigured = view.readiness?.configured ?? status?.configured ?? true;
  const defaultTrigger = view.readiness?.defaultTrigger ?? (scoutUnavailable ? "direct_batch" : "scout_then_concierge");
  const defaultMicTap = status?.defaultMicTap ?? view.readiness?.defaultMicTap ?? false;
  const needsSetup = phase === "setup" || view.readiness?.asr.state === "unavailable" ||
    (view.readiness?.concierge ?? status?.concierge) === "unavailable";
  const conciergeAttached = Boolean(status && status.binding.target.kind !== "detached");
  const conciergeReady = Boolean(view.loaded && isConfigured && !needsSetup && conciergeAttached && status?.concierge === "ready");
  const needsConcierge = Boolean(view.loaded && !conciergeReady && (status || needsSetup || !isConfigured));
  const routeRange = contiguousSpeechRange(chosen);

  function toggleSettings() {
    setSettings(value => !value);
    if (!settings) view.refreshReadiness();
  }

  function setUpWithGeneral() {
    const state = store.getState();
    // Live Mode on General itself would cover the conversation that does the setup, so it closes first.
    if (generalAgentTab(state)?.tabId === owner.id && !active) {
      onClose();
      void askGeneralAgent(state, GENERAL_SETUP_PROMPT).catch(() => undefined);
      return;
    }
    void view.mutate(() => askGeneralAgent(state, GENERAL_SETUP_PROMPT));
  }

  useEffect(() => {
    if (autoStartCount > 0) setPendingAutoStart(true);
  }, [autoStartCount]);

  useEffect(() => {
    if (!pendingAutoStart || !conciergeReady) return;
    setPendingAutoStart(false);
    if (capture.phase === "idle" || capture.phase === "paused") {
      void capture.start(canResume);
    }
  }, [pendingAutoStart, conciergeReady, capture.phase, capture.start, canResume]);

  const lastAutoAttach = useRef(0);
  const autoAttach = view.loaded && !needsSetup && isConfigured && defaultMicTap && status?.binding.target.kind === "detached";
  const { busy, mutate } = view;
  useEffect(() => {
    if (!autoAttach || busy || !autoStartCount || lastAutoAttach.current === autoStartCount || !pendingAutoStart) return;
    lastAutoAttach.current = autoStartCount;
    void mutate(signal => api.attach({ trigger: defaultTrigger }, signal));
  }, [api, autoAttach, autoStartCount, busy, defaultTrigger, mutate, pendingAutoStart]);

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
    data-drag-over={attachments.webDragOver} data-live={active} style={{ left: geometry.bounds.left, top: geometry.bounds.top }}
    {...attachments.webDropHandlers} {...geometry.drag()} onPaste={event => void attachments.handlePasteEvent(event.nativeEvent)}>
    <button type="button" className="ct-live-circle" aria-label="Expand Live Transcript" data-live={active}
      onClick={() => setCollapsedAt(undefined)}><AudioLines size={17} aria-hidden /></button>
    <button type="button" className="ct-live-orb-label" onClick={focusOwner}>{active ? "Live" : "Transcript"} · {ownerName}</button>
  </aside>;

  const hasEarlier = (view.history?.from ?? 1) > 1;
  const hasLater = page?.nextFromSeq != null;

  return <aside ref={attachments.container} className="ct-live-panel" style={geometry.bounds} aria-label={`Live Transcript for ${ownerName}`}
    data-drag-over={attachments.webDragOver} {...attachments.webDropHandlers}
    onPaste={event => void attachments.handlePasteEvent(event.nativeEvent)}>
    <header className="ct-live-header" {...geometry.drag({ ignore: "button:not(.ct-live-owner):not(.ct-live-handle), [role='group']" })}>
      <button type="button" className="ct-live-handle ct-live-icon" aria-label="Move Live Transcript" title="Move with the arrow keys or drag"
        onKeyDown={geometry.handle("move").onKeyDown}><GripVertical size={14} aria-hidden /></button>
      <button type="button" className="ct-live-owner" onClick={focusOwner} title={`Go to ${ownerName}`}>
        <span className="ct-live-dot" data-live={active} />
        <span className="ct-live-owner-state">{active ? "Live" : "Transcript"}</span>
        <span className="ct-live-owner-sep" aria-hidden> · </span>
        <span className="ct-live-owner-name">{ownerName}</span>
      </button>
      <Segmented className="ct-live-switch" aria-label="Transcript view" height={26} value={log ? "log" : "text"}
        options={VIEW_OPTIONS} onChange={value => switchView(value === "log")} />
      <button type="button" className="ct-live-icon" aria-label="Transcript settings" aria-expanded={settings} onClick={toggleSettings}>
        <Settings2 size={15} aria-hidden />
      </button>
      <button type="button" className="ct-live-icon" aria-label="Collapse Live Transcript" onClick={() => { setThoughts(undefined); setCollapsedAt(openCount); }}><Minus size={15} aria-hidden /></button>
      <button type="button" className="ct-live-icon" aria-label="Close Live Transcript" disabled={active} onClick={onClose}><X size={15} aria-hidden /></button>
    </header>
    {conciergeAttached && status && <ConciergeStatusBar status={status} settings={settings} onSettings={toggleSettings}
      thoughtsOpen={Boolean(log && thoughts?.epoch === ownEpoch && ownEpoch)}
      onThoughts={ownEpoch ? () => {
        switchView(true);
        if (!log || thoughts?.epoch !== ownEpoch) setThoughts({ epoch: ownEpoch });
      } : undefined} />}
    {(settings || needsConcierge || scoutUnavailable || status?.lastRouteError || capture.error || view.error) && <div className="ct-live-controls">
      {status?.lastRouteError && <div className="ct-live-callout ct-live-route-error">
        <span role="alert">{status.lastRouteError}</span>
        <button type="button" className="ct-live-ghost" onClick={() => { setSettings(true); view.refreshReadiness(); }}>Settings</button>
      </div>}
      {needsConcierge && !settings && <ConciergeSetupGate view={view} api={api} needsSetup={needsSetup}
        isConfigured={isConfigured} defaultTrigger={defaultTrigger} scoutUnavailable={scoutUnavailable}
        onSetup={setUpWithGeneral} onConnected={() => { setPendingAutoStart(true); setFeedback(undefined); }} />}
      {settings && <RoutingSettings owner={owner.id} view={view} api={api} autoResume={autoResume}
        onAutoResume={onAutoResume} scoutUnavailable={scoutUnavailable} defaultTrigger={defaultTrigger}
        defaultMicTap={defaultMicTap} isConfigured={isConfigured} needsSetup={needsSetup} />}
      {scoutUnavailable && !needsConcierge && <div className="ct-live-callout" role="status">
        Scout is unavailable. Choose Wake word, Direct batch, or Manual to route speech.
        {!settings && <button type="button" onClick={() => setSettings(true)}>Choose routing mode</button>}
      </div>}
      {(capture.error || view.error) && <p role="alert" className="ct-live-callout">{capture.error || view.error}</p>}
    </div>}
    <div ref={view.scrollRef} className="ct-live-document" onScroll={view.onScroll} tabIndex={0}
      data-selecting={chosen.length > 0} aria-label={log ? "Routing timeline" : "Transcript text"}>
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
          {row.content.kind === "speech" ? <label className="ct-live-speech" data-selected={selected.has(row.seq)}>
            <input type="checkbox" checked={selected.has(row.seq)} onChange={() => toggleRow(row.seq)}
              aria-label={`Select speech at ${clock(row.atMs)} (segment ${row.seq})`} />
            <span className="ct-live-speech-text">{row.content.text}</span>
            <time className="ct-live-speech-time" dateTime={new Date(row.atMs).toISOString()}>{clock(row.atMs)}</time>
          </label> : <div className="ct-live-marker">
            <span>{row.content.kind === "paused" ? "Capture paused" : "Capture resumed"} · {clock(row.atMs)}</span>
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
    {!log && (hasEarlier || hasLater) && <nav className="ct-live-toolbar ct-live-paging" aria-label="Transcript pages">
      <button type="button" className="ct-live-ghost" aria-label="Earlier speech" disabled={!hasEarlier}
        onClick={() => { setSelected(new Set()); view.moveHistory(Math.max(1, (view.history?.from ?? 1) - TRANSCRIPT_PAGE_SIZE)); }}>
        <ChevronUp size={13} aria-hidden />Earlier</button>
      <button type="button" className="ct-live-ghost" aria-label="Later speech" disabled={!hasLater} onClick={() => {
        if (page?.nextFromSeq != null) { setSelected(new Set()); view.moveHistory(page.nextFromSeq); }
      }}><ChevronDown size={13} aria-hidden />Later</button>
      {hasLater && <button type="button" className="ct-live-ghost ct-live-paging-end" aria-label="Latest speech"
        onClick={() => { setSelected(new Set()); view.moveHistory(null); }}><ChevronsDown size={13} aria-hidden />Latest</button>}
    </nav>}
    {!log && selectedText && <div className="ct-live-toolbar ct-live-selection" role="toolbar" aria-label="Selected speech">
      <span className="ct-live-selection-count">{chosen.length} selected</span>
      <button type="button" className="ct-live-ghost" onClick={exportSelected}><Download size={13} aria-hidden />Export</button>
      <button type="button" className="ct-live-ghost" disabled={view.busy} onClick={async () => {
        if (await view.mutate(signal => api.save("Live Transcript selection", selectedText, signal))) setFeedback("Selection saved to Memory.");
      }}>Save to Memory</button>
      {status?.trigger === "manual" && <button type="button" className="ct-live-ghost" disabled={view.busy || !routeRange}
        onClick={() => { if (routeRange) void view.mutate(signal => api.route(routeRange, signal)); }}>Route selection</button>}
      <button type="button" className="ct-live-icon ct-live-selection-clear" aria-label="Clear selection"
        onClick={() => setSelected(new Set())}><X size={13} aria-hidden /></button>
    </div>}
    <footer className="ct-live-footer">
      <span role="status" className="ct-live-status"><span className="ct-live-dot" data-live={active} />
        {feedback ?? (needsConcierge && !active ? "Connect a Concierge to start" : captureLabel(phase))}</span>
      <button type="button" className="ct-live-icon" aria-label="Refresh transcript" title="Refresh transcript" onClick={view.refreshReadiness}>
        <RefreshCw size={13} aria-hidden />
      </button>
      {active ? <button type="button" className="ct-live-cta" data-variant="pause" disabled={phase === "pausing"} onClick={() => void capture.pause()}>
        <Pause size={13} aria-hidden />Pause</button> :
        <button type="button" className="ct-live-cta" disabled={!conciergeReady || view.busy} title={!conciergeReady ? "Connect a Concierge before starting live capture" : undefined} onClick={() => void capture.start(canResume)}>
          <Mic size={13} aria-hidden />{canResume ? "Resume" : "Start"}
        </button>}
      <button type="button" className="ct-live-resize" aria-label="Resize Live Transcript" title="Resize with the arrow keys or drag"
        {...geometry.handle("size")}><svg viewBox="0 0 10 10" width="10" height="10" aria-hidden><path d="M9 3 3 9M9 6.5 6.5 9" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" fill="none" /></svg></button>
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
  const options: DropdownSelectOption[] = [{ value: "", label: "Current epoch" }, ...records.map(item => ({
    value: item.action.actionId,
    label: `${clock(item.action.atMs)} · Speech ${item.action.range.fromSeq}${item.binding.target.kind !== "attached" ? " · External" : ""}`,
  }))];
  return <>
    <span className="ct-live-select ct-live-select--inline">
      <DropdownSelect aria-label="Thoughts for" data-testid="live-transcript-thoughts-for" selectionIndicator="highlight"
        options={options} value={record ? receipt : ""} menuMaxHeight={260}
        onChange={value => { setReceipt(value); onOpen(undefined); }} />
    </span>
    <button type="button" className="ct-live-ghost" aria-expanded={Boolean(open)} disabled={!epoch}
      onClick={() => onOpen(open ? undefined : epoch ? { epoch, rationale: record?.action.rationale } : undefined)}>
      Thoughts <ChevronDown size={13} aria-hidden />
    </button>
    {!epoch && <span className="ct-live-muted">Reasoning unavailable</span>}
  </>;
}

function conciergeSummary(router: TranscriptReadiness["router"], trigger: Trigger) {
  const identity = [router?.provider, router?.model].filter(Boolean).join(" / ");
  return [identity, MODES[trigger]].filter(Boolean).join(" · ");
}

const ACTIVITY: Record<RouterActivity, string> = {
  idle: "Watching",
  reviewing: "Reviewing…",
  stuck: "Stalled",
  unavailable: "Unavailable",
};

function ConciergeStatusBar({ status, settings, onSettings, thoughtsOpen, onThoughts }: {
  status: TranscriptOverview["status"];
  settings: boolean;
  onSettings: () => void;
  thoughtsOpen: boolean;
  onThoughts?: () => void;
}) {
  const target = status.binding.target;
  const external = useMuxSelector(state => target.kind === "external" ? state.tabs[target.tabId] : undefined);
  const identity = target.kind === "external"
    ? `${external && isSidebarTabVisible(external) ? displayTitleForTab(external) : "External tab"} · ${MODES[status.trigger]}`
    : `Concierge · ${conciergeSummary(status.router, status.trigger)}`;
  const activity = status.router?.activity ?? (status.concierge === "unavailable" ? "unavailable" : "idle");
  const visibleActivity = status.awaitingDelivery && activity === "idle" ? "reviewing" : activity;
  return <div className="ct-live-concierge-bar" data-testid="live-transcript-concierge-bar" data-activity={visibleActivity}>
    <button type="button" className="ct-live-concierge-identity" title={identity} aria-expanded={settings} onClick={onSettings}>
      <span className="ct-live-dot" aria-hidden /><span>{identity}</span>
    </button>
    <span role="status" className="ct-live-concierge-activity">{ACTIVITY[visibleActivity]}</span>
    {onThoughts && <button type="button" className="ct-live-ghost" aria-label="Open current Concierge thoughts"
      aria-expanded={thoughtsOpen} onClick={onThoughts}>Thoughts</button>}
  </div>;
}

function ConciergeSetupGate({ view, api, needsSetup, isConfigured, defaultTrigger, scoutUnavailable, onSetup, onConnected }: {
  view: ReturnType<typeof useLiveTranscriptPanel>;
  api: LiveTranscriptApi;
  needsSetup: boolean;
  isConfigured: boolean;
  defaultTrigger: Trigger;
  scoutUnavailable: boolean;
  onSetup: () => void;
  onConnected: () => void;
}) {
  const [manual, setManual] = useState(false);
  const [mode, setMode] = useState<Trigger>();
  const trigger = mode ?? defaultTrigger;
  const router = view.readiness?.router ?? view.overview?.status.router;
  const [choice, setChoice] = useState<ConciergeChoice>({});
  const selection = conciergeSelection(view.readiness, router, choice);
  const needsGeneral = needsSetup || !isConfigured;
  return <section className="ct-live-gate" aria-label="Live Transcript setup" data-testid="live-transcript-concierge-gate">
    <span className="ct-live-gate-mark" aria-hidden><AudioLines size={20} /></span>
    <h2 className="ct-live-gate-title">{needsGeneral ? "Set up Live Transcript" : "Concierge ready"}</h2>
    <p className="ct-live-gate-lead">
      {needsSetup
        ? "General helps install Parakeet speech recognition and connect Scout and Concierge."
        : !isConfigured
          ? "General sets up your lightweight Concierge (local LM Studio/Ollama Scout or cloud) so it filters chatter and routes only real requests."
          : conciergeSummary(router, defaultTrigger)}
    </p>
    {needsGeneral
      ? <button type="button" className="ct-live-primary" disabled={view.busy} onClick={onSetup}>Set up with General</button>
      : <button type="button" className="ct-live-primary" disabled={view.busy} onClick={async () => {
        if (await view.mutate(signal => api.attach({ trigger: defaultTrigger }, signal))) onConnected();
      }}><Mic size={14} aria-hidden />{view.busy ? "Starting…" : "Start Live Mode"}</button>}
    {!needsSetup && !isConfigured && <>
      <button type="button" className="ct-live-ghost" aria-expanded={manual} onClick={() => setManual(value => !value)}>
        Or connect manually <ChevronDown size={13} aria-hidden />
      </button>
      {manual && <div className="ct-live-manual">
        <div className="ct-live-fields">
          <ConciergePickers selection={selection} onChange={setChoice} />
          <RoutingModePicker trigger={trigger} scoutUnavailable={scoutUnavailable} onChange={setMode} />
        </div>
        <button type="button" className="ct-live-ghost" disabled={view.busy || !selection.provider || (scoutUnavailable && trigger === "scout_then_concierge")}
          onClick={async () => {
            if (await view.mutate(signal => api.attach({
              trigger, provider: selection.provider, model: selection.model || undefined, configure: true,
            }, signal))) onConnected();
          }}>Connect Concierge &amp; Start</button>
      </div>}
    </>}
  </section>;
}

const GENERAL_SETUP_PROMPT = "Set up Live Transcript for me (clicking Set up with General is my consent to install what dictation needs). Keep it fast: skip `mem search`, `domios docs`, `--help`, `channel list/diff`, `plugin get`, and `domios live-transcript status` (live-transcript status is owner-only); plugin verbs take no `--json`.\n" +
  "1. Speech: `domios plugin channel refresh codeterm-plugins`, then `domios plugin update transcriber` (if it is missing or local, `domios plugin install transcriber --channel codeterm-plugins`). Transcriber must be 0.5.1 or newer. Then `domios plugin transcriber dictation-status`; if it is not ready, run `domios plugin transcriber dictation-install parakeet-tdt-0.6b-v3-int8 --consent`, follow `domios plugin transcriber dictation-progress` until it completes, then `domios plugin transcriber dictation-select parakeet-tdt-0.6b-v3-int8` and `domios plugin transcriber dictation-test parakeet-tdt-0.6b-v3-int8`.\n" +
  "2. Routing: after checking ASR with dictation-test, help me choose, configure, and verify a Scout (local LM Studio/Ollama or cloud) and a Concierge provider/model. Explain the model size for local routing and any costs for cloud routing. Save my chosen `live_transcript.trigger`, `live_transcript.provider`, and `live_transcript.model` if customized. Once ASR and the chosen routing are ready, run `domios config set live_transcript.configured true`.\n" +
  "3. Reply concisely in my language: confirm Live Mode is ready (or give the exact error if a step failed), then explain in a few sentences what Live Transcript is and how to use it: open Transcript and click Start Live Mode (or swipe up on the mic); it listens locally, ignores chatter and routes only actionable requests; a floating Orb shows its state; a local Scout model is private but needs disk space, a cloud one costs more. Never touch voice-note settings or existing drafts.";

type ConciergeChoice = { provider?: string; model?: string };

function conciergeSelection(readiness: TranscriptReadiness | undefined, router: TranscriptReadiness["router"], choice: ConciergeChoice) {
  const providers = readiness?.router?.providers ?? [];
  const provider = choice.provider ?? router?.provider ?? providers[0]?.id ?? "";
  const catalog = providers.find(option => option.id === provider);
  const model = choice.model ?? (provider === router?.provider ? router.model : undefined) ?? catalog?.defaultModel ?? "";
  const providerOptions = providers.map(option => ({ value: option.id, label: option.displayName }));
  if (provider && !catalog) providerOptions.unshift({ value: provider, label: provider });
  const models = new Set(catalog?.models);
  if (catalog?.defaultModel) models.add(catalog.defaultModel);
  if (model) models.add(model);
  const modelOptions = [{ value: "", label: "Provider default" }, ...Array.from(models, value => ({ value, label: value }))];
  return { provider, model, providerOptions, modelOptions };
}

function ConciergePickers({ selection, onChange }: {
  selection: ReturnType<typeof conciergeSelection>;
  onChange: (choice: ConciergeChoice) => void;
}) {
  return <>
    <div className="ct-live-field"><span aria-hidden>Concierge provider</span>
      <DropdownSelect aria-label="Concierge provider" data-testid="live-transcript-concierge-provider" selectionIndicator="highlight"
        options={selection.providerOptions} value={selection.provider} placeholder="No providers available"
        disabled={!selection.providerOptions.length} onChange={provider => onChange({ provider })} />
    </div>
    <div className="ct-live-field"><span aria-hidden>Concierge model</span>
      <DropdownSelect aria-label="Concierge model" data-testid="live-transcript-concierge-model" selectionIndicator="highlight"
        options={selection.modelOptions} value={selection.model} disabled={!selection.provider}
        onChange={model => onChange({ provider: selection.provider, model })} />
    </div>
  </>;
}

function RoutingModePicker({ trigger, scoutUnavailable, onChange }: {
  trigger: Trigger;
  scoutUnavailable: boolean;
  onChange: (trigger: Trigger) => void;
}) {
  return <div className="ct-live-field"><span aria-hidden>Routing mode</span>
    <DropdownSelect aria-label="Routing mode" data-testid="live-transcript-routing-mode" selectionIndicator="highlight"
      options={modeOptions(scoutUnavailable)} value={trigger} onChange={value => onChange(value as Trigger)} />
  </div>;
}

function RoutingSettings({ owner, view, api, autoResume, onAutoResume, scoutUnavailable, defaultTrigger, defaultMicTap, isConfigured, needsSetup }: {
  owner: string;
  view: ReturnType<typeof useLiveTranscriptPanel>;
  api: LiveTranscriptApi;
  autoResume: boolean;
  onAutoResume: (value: boolean) => void;
  scoutUnavailable: boolean;
  defaultTrigger: Trigger;
  defaultMicTap: boolean;
  isConfigured: boolean;
  needsSetup: boolean;
}) {
  const tabs = useMuxSelector(state => state.tabs);
  const status = view.overview?.status;
  const target = status?.binding.target;
  const configuredTab = target?.kind === "external" ? target.tabId : "";
  const [choice, setChoice] = useState<{ trigger?: Trigger; externalTab?: string }>({});
  const [conciergeChoice, setConciergeChoice] = useState<ConciergeChoice>({});
  const micHint = useId();
  const trigger = choice.trigger ?? (target?.kind !== "detached" ? status?.trigger : undefined) ?? defaultTrigger;
  const externalTab = choice.externalTab ?? configuredTab;
  const externalTabs = Object.values(tabs).filter(tab => tab.id !== owner && usableTab(tab) && tab.agent_type);
  const router = (!configuredTab ? status?.router : null) ?? view.readiness?.router;
  const concierge = conciergeSelection(view.readiness, router, conciergeChoice);
  const savedConcierge = conciergeSelection(view.readiness, router, {});
  const conciergeChanged = !externalTab && (concierge.provider !== savedConcierge.provider || concierge.model !== savedConcierge.model);
  const reconnect = status?.concierge !== "ready" || Boolean(status?.lastRouteError);
  const canApply = isConfigured && !needsSetup && status &&
    (reconnect || target?.kind === "detached" || externalTab !== configuredTab || trigger !== status.trigger || conciergeChanged) &&
    !(scoutUnavailable && trigger === "scout_then_concierge") && (!externalTab || externalTabs.some(tab => tab.id === externalTab));
  const routeOptions: DropdownSelectOption[] = [
    { value: "", label: "Transcript Concierge", badge: "Built-in" },
    ...(externalTab && !externalTabs.some(tab => tab.id === externalTab)
      ? [{ value: externalTab, label: "Unavailable external tab", unavailable: true }] : []),
    ...externalTabs.map(tab => ({ value: tab.id, label: displayTitleForTab(tab) })),
  ];
  return <section className="ct-live-settings" aria-label="Routing settings">
    <div className="ct-live-fields">
      <RoutingModePicker trigger={trigger} scoutUnavailable={scoutUnavailable}
        onChange={value => setChoice(choice => ({ ...choice, trigger: value }))} />
      <div className="ct-live-field"><span aria-hidden>Route to</span>
        <DropdownSelect aria-label="Route to" data-testid="live-transcript-route-to" selectionIndicator="highlight"
          options={routeOptions} value={externalTab} menuMaxHeight={260}
          onChange={value => setChoice(choice => ({ ...choice, externalTab: value }))} />
      </div>
      {!externalTab && <ConciergePickers selection={concierge} onChange={setConciergeChoice} />}
    </div>
    {isConfigured && <div className="ct-live-toggle-row">
      <div><span>Tap mic starts Live Transcript</span>
        <small id={micHint}>Hold mic still records a regular voice note. Slide up locks Live Transcript.</small>
      </div>
      <LiquidToggle aria-label="Tap mic starts Live Transcript" aria-describedby={micHint} checked={defaultMicTap}
        disabled={!status || view.busy} onCheckedChange={next => {
          if (status) void view.mutate(signal => api.attach({ modeOnly: true, trigger: status.trigger, defaultMicTap: next }, signal));
        }} />
    </div>}
    <div className="ct-live-toggle-row">
      <span>Pause when this window loses focus, resume on return</span>
      <LiquidToggle aria-label="Pause when this window loses focus and resume on return" checked={autoResume} onCheckedChange={onAutoResume} />
    </div>
    <div className="ct-live-settings-actions">
      <button type="button" className="ct-live-primary ct-live-primary--compact" disabled={!canApply || view.busy} onClick={() => {
        void view.mutate(signal => api.attach(externalTab
          ? externalTab === configuredTab && !reconnect ? { modeOnly: true, trigger } : { trigger, externalTab }
          : { trigger, provider: concierge.provider || undefined, model: concierge.model || undefined, configure: true }, signal));
      }}>Apply routing</button>
    </div>
  </section>;
}
