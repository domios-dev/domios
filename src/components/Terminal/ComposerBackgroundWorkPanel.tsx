import { useId, useRef, useState } from "react";
import { Bot, ChevronDown, Square, SquareTerminal } from "lucide-react";
import { useAgentInspection } from "@codeterm/chat-ui";
import { isRunningWork, SUBAGENT_NAME, type ComposerBackgroundWorkItem } from "./composerBackgroundWorkModel";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { subagentTone } from "./childTranscriptViewModel";
import type { BackgroundStopRequestStatus } from "../../lib/backgroundProcesses";

const NO_LIVE_EVIDENCE = "No live evidence";
const MAX_VISIBLE_PROCESSES = 6;

function stateLabel(item: ComposerBackgroundWorkItem): string {
  if (item.status === "unknown" || !item.status) return "unknown";
  return item.elapsed ? `running · ${item.elapsed}` : "running";
}

function displayName(item: ComposerBackgroundWorkItem): string {
  return item.ordinal ? `${item.name} ${item.ordinal}` : item.name;
}

function describe(item: ComposerBackgroundWorkItem): string {
  return [`Task ${displayName(item)}, ${item.status === "unknown" ? NO_LIVE_EVIDENCE.toLowerCase() : stateLabel(item)}`, item.model, item.effort, item.rawId && `id ${item.rawId}`].filter(Boolean).join(". ");
}

function WorkState({ item, label, stopping = false }: { item: ComposerBackgroundWorkItem; label?: string; stopping?: boolean }) {
  const unknown = item.status === "unknown" || !item.status;
  const state = stopping ? "stopping" : unknown ? "unknown" : "running";
  const text = stopping ? "stopping…" : (label ?? stateLabel(item)).replace(/^running · /, "");
  return <span className="ct-work-state" data-state={state} title={!stopping && unknown ? NO_LIVE_EVIDENCE : undefined}>
    <span className="ct-work-state__dot" aria-hidden="true" />{text}
  </span>;
}

function SubagentRow({ item, internalAgentLabel }: { item: ComposerBackgroundWorkItem; internalAgentLabel: string }) {
  const inspect = useAgentInspection();
  const [expanded, setExpanded] = useState(false);
  const promptId = useId();
  return <li className="ct-background-command">
    <span className="ct-ctx-bubble ct-work-bubble" data-tone={subagentTone(item.status)}>
      <button type="button" className="ct-ctx-bubble__open" aria-label={`Open transcript for ${item.name}, ${internalAgentLabel}`} aria-description={describe(item)}
        disabled={!inspect || !item.transcriptThreadId} title={`${describe(item)} · ${internalAgentLabel}`} onClick={() => { if (item.transcriptThreadId) inspect?.(item.transcriptThreadId); }}>
        <Bot size={13} aria-hidden="true" />
        <span className="ct-ctx-bubble__title">{item.name === SUBAGENT_NAME ? item.name : `${SUBAGENT_NAME} · ${item.name}`}</span>
        {item.ordinal && <span className="ct-work-bubble__ordinal" aria-hidden="true">{item.ordinal}</span>}
        <WorkState item={item} />
      </button>
      {item.prompt && <button type="button" className="ct-ctx-bubble__dismiss" aria-label={`Task details for ${item.name}`} aria-expanded={expanded} aria-controls={promptId} title="Task details" onClick={() => setExpanded(!expanded)}>
        <ChevronDown size={11} className={expanded ? "rotate-180" : undefined} aria-hidden="true" />
      </button>}
    </span>
    {expanded && item.prompt && <div id={promptId} role="region" aria-label={`Task details for ${item.name}`} className="ct-background-command__details ct-background-command__prompt">{item.prompt}</div>}
  </li>;
}

function SubagentDetails({ items, internalAgentLabel }: { items: readonly ComposerBackgroundWorkItem[]; internalAgentLabel: string }) {
  if (!items.length) return null;
  return <ul aria-label="Subagents" className="ct-background-commands">
    {items.map(item => <SubagentRow key={item.id} item={item} internalAgentLabel={internalAgentLabel} />)}
  </ul>;
}

type WorkActions = {
  onStop: (item: ComposerBackgroundWorkItem) => Promise<void>;
  onViewOutput?: (item: ComposerBackgroundWorkItem) => Promise<string | undefined>;
  onAskStop?: (item: ComposerBackgroundWorkItem) => Promise<BackgroundStopRequestStatus>;
  agentName?: string;
};

const requestLabels: Record<BackgroundStopRequestStatus, string> = {
  pending: "Stop request queued",
  unconfirmed: "Stop request delivery unconfirmed",
  written: "Stop request written",
};

function WorkDetails({ item, onStop, onViewOutput, onAskStop, agentName }: WorkActions & { item: ComposerBackgroundWorkItem }) {
  const [busy, setBusy] = useState<"stop" | "output" | "ask" | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [outputOpen, setOutputOpen] = useState(false);
  const [output, setOutput] = useState<string>();
  const [request, setRequest] = useState<BackgroundStopRequestStatus>();
  const detailsId = useId();
  const outputId = useId();
  const [error, setError] = useState<{ message: string; operation: string } | null>(null);
  const inFlight = useRef(false);
  const live = isRunningWork(item);
  const unknownStatus = item.status === "unknown" || !item.status;
  const unknownKind = item.kind === "unknown";
  const processes = item.processes ?? [];
  const helperCount = item.helperCount ?? Math.max(0, processes.length - 1);
  const totalProcessCount = Math.max(1, processes.length || helperCount + 1);
  const visibleProcesses = processes.slice(0, MAX_VISIBLE_PROCESSES);
  const hiddenProcessCount = Math.max(0, processes.length - MAX_VISIBLE_PROCESSES);
  const showProcessList = processes.length > 1 || (processes.length === 1 && processes[0].role === "helper");
  const stopLabel = helperCount > 0
    ? `Stop ${item.name} and ${helperCount} ${helperCount === 1 ? "helper" : "helpers"}`
    : `Stop ${item.name}`;
  const helperBadgeLabel = `${helperCount} helper ${helperCount === 1 ? "process" : "processes"}`;
  const run = async (action: Exclude<typeof busy, null>, operation: string, apply: () => Promise<void>) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(action);
    setError(null);
    try {
      await apply();
    } catch (reason) {
      setError({ message: reason instanceof Error ? reason.message : String(reason), operation });
    } finally {
      inFlight.current = false;
      setBusy(null);
    }
  };
  const toggleOutput = () => {
    const next = !outputOpen;
    setOutputOpen(next);
    if (next) void run("output", "viewing background process output", async () => {
      setOutput(onViewOutput ? await onViewOutput(item) : item.outputTail);
    });
  };

  return <li className="ct-background-command">
      <span className="ct-ctx-bubble ct-work-bubble" data-tone={subagentTone(item.status)}>
        <span className="ct-ctx-bubble__open" title={item.command ?? item.name}>
          <SquareTerminal size={13} aria-hidden="true" />
          <span className="ct-ctx-bubble__title">{item.name}</span>
          {helperCount > 0 && <span className="ct-work-bubble__ordinal" aria-label={helperBadgeLabel}>+{helperCount}</span>}
          <WorkState item={item} stopping={busy === "stop"} />
        </span>
        <button type="button" className="ct-ctx-bubble__dismiss" aria-label={`Details for ${item.name}`} aria-expanded={detailsOpen} aria-controls={detailsId} title="Details"
          onClick={() => setDetailsOpen(!detailsOpen)}>
          <ChevronDown size={11} className={detailsOpen ? "rotate-180" : undefined} aria-hidden="true" />
        </button>
        {item.kind === "terminal" && <button type="button" className="ct-ctx-bubble__dismiss" aria-label={stopLabel} title="Stop" disabled={busy !== null} aria-busy={busy === "stop" || undefined}
          onClick={() => void run("stop", "stopping the background process", () => onStop(item))}><Square size={10} fill="currentColor" aria-hidden="true" /></button>}
      </span>
      {detailsOpen && <div id={detailsId} role="region" aria-label={`Details for ${item.name}`} className="ct-background-command__details" aria-busy={busy === "output" || undefined}>
        {unknownStatus && <p className="ct-background-command__note">No live evidence for this process.</p>}
        {item.command && <code className="ct-background-command__cmd">{item.command}</code>}
        {showProcessList && <ul className="ct-background-command__processes" aria-label={`Processes for ${item.name}`}>
          {visibleProcesses.map((proc, index) => {
            const meta = proc.role === "helper"
              ? proc.osPid != null ? `helper · pid ${proc.osPid}` : "helper"
              : proc.osPid != null ? `pid ${proc.osPid}` : "";
            return <li key={`${proc.processId ?? proc.osPid ?? index}:${proc.role}`} className="ct-background-command__proc" data-role={proc.role}>
              <span className="ct-background-command__proc-cmd" title={proc.command}>{proc.command}</span>
              {meta && <span className="ct-background-command__proc-meta">{meta}</span>}
            </li>;
          })}
          {hiddenProcessCount > 0 && <li className="ct-background-command__proc ct-background-command__proc--more">+{hiddenProcessCount} more</li>}
        </ul>}
        {(!unknownKind || (item.kind === "terminal" && totalProcessCount > 1)) && <div className="ct-background-command__actions">
          {!unknownKind && <button type="button" className="underline" aria-label={`${outputOpen ? "Hide" : "View"} output for ${item.name}`} aria-expanded={outputOpen} aria-controls={outputId} disabled={busy !== null} onClick={toggleOutput}>
            {outputOpen ? "Hide output" : "View output"}
          </button>}
          {item.kind === "terminal" && totalProcessCount > 1 && <span>Stop ends all {totalProcessCount}</span>}
          {item.kind === "shell" && (live || unknownStatus) && <>
            <span>Stop unavailable</span>
            {live && onAskStop && <button type="button" className="underline" disabled={busy !== null || request !== undefined}
              onClick={() => void run("ask", "requesting a background process stop", async () => setRequest(await onAskStop(item)))}>
              Ask {agentName || "agent"} to stop it
            </button>}
            {request && <span role="status">{requestLabels[request]}</span>}
          </>}
        </div>}
        {outputOpen && <div id={outputId} className="ct-background-command__output">
          {output ? <pre>{output}</pre> : busy !== "output" && !error && <p>No output captured yet</p>}
        </div>}
      </div>}
      {error && <div role="alert" className="mt-1"><InlineErrorNotice error={error.message} operation={error.operation} /></div>}
  </li>;
}

export function ComposerBackgroundWorkPanel({ items, internalAgentLabel, ...actions }: WorkActions & {
  items: readonly ComposerBackgroundWorkItem[];
  internalAgentLabel: string;
}) {
  return <div>
    <SubagentDetails items={items.filter(item => item.kind === "agent")} internalAgentLabel={internalAgentLabel} />
    <ul className="ct-background-commands">
      {items.filter(item => item.kind !== "agent").map(item => <WorkDetails key={item.id} item={item} {...actions} />)}
    </ul>
  </div>;
}
