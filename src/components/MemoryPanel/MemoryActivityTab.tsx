import { InlineErrorNotice } from "../shared/ErrorNotice";
import { memo, useState } from "react";
import { ChevronRight, Moon, Network, Plus, RefreshCw } from "lucide-react";
import type { ApiConfig, AppConfig, GraphWorkerInfo } from "../../lib/types";
import { useAgentDescriptors } from "../../lib/useAgentDescriptor";
import type { MemNode } from "./types";
import { useGraphWorkers } from "./useGraphWorkers";
import { useMemoryActivity } from "./useMemoryActivity";
import { MemorySpawnWorkerDialog } from "./MemorySpawnWorkerDialog";
import { AgentRunLogPopup } from "../AgentRun/AgentRunLogPopup";
import { RunChatPopup } from "../AgentRun/RunChatPopup";
import { defaultSpawnProviderId } from "../../lib/agentDescriptorCache";
import { isTerminalWorkerStatus } from "../AgentRun/workerEmptyState";
import {
  agentActivity,
  workerAgentKind,
  workerFailureReason,
  workerNeedsAttention,
  workerOutcome,
  workerTaskLabel,
  workerTimeLabels,
  type MemoryAgentKind,
} from "./workerActivityModel";
import { MemoryActivityServices } from "./MemoryActivityServices";

interface Props {
  api: ApiConfig;
  config?: AppConfig;
  nodes: Map<string, MemNode>;
  onNavigateToItem: (id: string) => void;
}

export const activityButton =
  "inline-flex items-center justify-center gap-2 rounded px-3 py-1.5 text-xs border border-[var(--ct-border)] bg-transparent text-[var(--ct-fg)] cursor-pointer hover:bg-[var(--ct-surface)] focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]";
const muted = "text-xs text-[var(--ct-muted-fg)]";
const agents = [
  {
    kind: "dreaming",
    title: "Dreaming",
    description: "Consolidates memories and writes a dream diary.",
    Icon: Moon,
  },
  {
    kind: "extraction",
    title: "Extraction",
    description: "Turns saved memories into connected knowledge.",
    Icon: Network,
  },
] as const;

function MemoryActivityTabImpl({ api, config, nodes, onNavigateToItem }: Props) {
  const { workers, loading, error, spawn, kill, refresh } = useGraphWorkers(api);
  const [revision, setRevision] = useState(0);
  const activity = useMemoryActivity(api, revision);
  const [tab, setTab] = useState<"workers" | "services">("workers");
  const [selected, setSelected] = useState<MemoryAgentKind | null>(null);
  const [spawnOpen, setSpawnOpen] = useState(false);
  const [logWorker, setLogWorker] = useState<GraphWorkerInfo | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const providers = useAgentDescriptors();
  const dreamCron = activity.crons.data?.find((cron) => cron.name === "mem-dreaming-auto");
  const selectedWorkers = workers.filter((worker) => workerAgentKind(worker) === selected);
  const history = agentActivity(selectedWorkers).history;
  const taskLabel = (worker: GraphWorkerInfo) =>
    workerTaskLabel(worker, nodes.get(worker.intent.replace(/^extract:/, ""))?.title);

  async function stopWorker(workerId: string) {
    setActionError(null);
    try {
      await kill(workerId);
      await refresh();
    } catch (failure) {
      setActionError(String(failure));
    }
  }

  return (
    <div
      className="absolute inset-0 flex flex-col text-[var(--ct-fg)]"
      data-testid="memory-activity-tab"
    >
      <div className="flex items-center gap-2 border-b border-[var(--ct-border)] px-4 py-3">
        <div role="tablist" aria-label="Memory activity" className="flex gap-1">
          {(["workers", "services"] as const).map((value) => (
            <button
              key={value}
              type="button"
              role="tab"
              id={`activity-${value}`}
              aria-selected={tab === value}
              aria-controls={`activity-${value}-panel`}
              className={`${activityButton} ${tab === value ? "bg-[var(--ct-surface)] font-semibold" : "border-transparent"}`}
              onClick={() => setTab(value)}
            >
              {value === "workers" ? "Workers" : "Services"}
            </button>
          ))}
        </div>
        <button
          type="button"
          className={`${activityButton} ml-auto`}
          aria-label="Refresh activity"
          onClick={() => {
            void refresh();
            setRevision((value) => value + 1);
          }}
        >
          <RefreshCw size={13} />
        </button>
        {tab === "workers" && (
          <button type="button" className={activityButton} onClick={() => setSpawnOpen(true)}>
            <Plus size={13} />
            New worker
          </button>
        )}
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto p-4 md:p-6">
        <div className="max-w-4xl mx-auto space-y-5">
          {tab === "workers" ? (
            <section
              role="tabpanel"
              id="activity-workers-panel"
              aria-labelledby="activity-workers"
              className="space-y-5"
            >
              <div>
                <h2 className="text-sm font-semibold mb-1">Memory agents</h2>
                <p className={muted}>
                  Configured here, working in the background. Open an agent to see its runs.
                </p>
              </div>
              {error && (
                <details className="text-xs text-red-400">
                  <summary className="cursor-pointer">
                    Worker status unavailable. Refresh to try again.
                  </summary>
                  <InlineErrorNotice error={error} operation="loading worker status" />
                </details>
              )}
              <div className="rounded-lg border border-[var(--ct-border)] overflow-hidden divide-y divide-[var(--ct-border)]">
                {agents.map(({ kind, title, description, Icon }) => {
                  const current = agentActivity(
                    workers.filter((worker) => workerAgentKind(worker) === kind),
                  );
                  const providerId =
                    config?.memory?.[kind === "dreaming" ? "dreaming_agent" : "extraction_agent"] ||
                    config?.general_agent?.agent_provider;
                  const provider = providers.find((item) => item.id === providerId);
                  const model =
                    config?.memory?.[kind === "dreaming" ? "dreaming_model" : "extraction_model"] ||
                    config?.memory?.worker_model;
                  const modelLabel =
                    provider?.available_models.find((item) => item.id === model)?.display_name ||
                    model ||
                    "Provider default";
                  const disabled =
                    kind === "extraction"
                      ? config?.memory?.auto_extract === false
                      : dreamCron?.enabled === false;
                  const failed =
                    (current.latest && workerNeedsAttention(current.latest)) ||
                    (kind === "dreaming" && dreamCron?.last_run?.status === "Failed");
                  const state = error
                    ? "Status unavailable"
                    : loading
                      ? "Loading"
                      : current.live.length
                        ? `${current.live.length} running${current.parked.length ? ` / ${current.parked.length} parked` : ""}`
                        : current.parked.length
                          ? `${current.parked.length} parked`
                          : failed
                            ? "Last run failed"
                            : disabled
                              ? "Automatic runs off"
                              : current.latest
                                ? `Last run ${workerOutcome(current.latest).toLowerCase()}`
                                : "Idle";
                  return (
                    <button
                      key={kind}
                      type="button"
                      aria-expanded={selected === kind}
                      aria-controls="memory-agent-details"
                      className="w-full text-left bg-transparent px-4 py-4 flex gap-3 items-start cursor-pointer hover:bg-[var(--ct-surface)] focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
                      onClick={() => setSelected(selected === kind ? null : kind)}
                    >
                      <Icon size={18} className="mt-0.5 shrink-0 text-[var(--ct-muted-fg)]" />
                      <span className="flex-1 min-w-0">
                        <span className="block text-sm font-medium">{title}</span>
                        <span className={`${muted} block mt-1`}>{description}</span>
                        <span className={`${muted} block mt-2`}>
                          {config
                            ? `${provider?.display_name || providerId || "Provider not configured"} / ${modelLabel}`
                            : "Configuration unavailable"}
                        </span>
                        {current.live[0] && (
                          <span className="block text-xs mt-2 truncate">
                            {current.live[0].progress_text || taskLabel(current.live[0])}
                          </span>
                        )}
                      </span>
                      <span
                        className={`text-xs shrink-0 ${failed || current.parked.length ? "text-red-400" : "text-[var(--ct-muted-fg)]"}`}
                      >
                        {state}
                      </span>
                      <ChevronRight size={14} className="shrink-0 mt-0.5" />
                    </button>
                  );
                })}
                {workers.some((worker) => workerAgentKind(worker) === "custom") && (
                  <button
                    type="button"
                    className="w-full bg-transparent text-left p-4 text-sm cursor-pointer hover:bg-[var(--ct-surface)]"
                    aria-expanded={selected === "custom"}
                    onClick={() => setSelected(selected === "custom" ? null : "custom")}
                  >
                    Other memory workers
                    <span className="ml-3 text-xs text-[var(--ct-muted-fg)]">
                      {
                        workers.filter(
                          (worker) =>
                            workerAgentKind(worker) === "custom" &&
                            !isTerminalWorkerStatus(worker.status),
                        ).length
                      }{" "}
                      running
                      {agentActivity(
                        workers.filter((worker) => workerAgentKind(worker) === "custom"),
                      ).latest &&
                      workers
                        .filter((worker) => workerAgentKind(worker) === "custom")
                        .some(workerNeedsAttention)
                        ? " / failures in history"
                        : ""}
                    </span>
                    <ChevronRight size={14} className="inline ml-2" />
                  </button>
                )}
              </div>
              {selected && (
                <section
                  id="memory-agent-details"
                  aria-label={`${selected} details`}
                  className="border border-[var(--ct-border)] rounded-lg p-4 space-y-4"
                >
                  <div className="flex justify-between items-center">
                    <h3 className="text-sm font-semibold">
                      {agents.find((agent) => agent.kind === selected)?.title || "Other workers"}{" "}
                      history
                    </h3>
                    <button
                      type="button"
                      className={activityButton}
                      onClick={() => setSelected(null)}
                    >
                      Close details
                    </button>
                  </div>
                  {selected === "dreaming" && dreamCron?.last_run && (
                    <p className={muted}>
                      Latest scheduled attempt: {dreamCron.last_run.status} /{" "}
                      {new Date(dreamCron.last_run.started_at).toLocaleString()}
                    </p>
                  )}
                  {selected === "dreaming" && (
                    <div className={muted}>
                      {activity.crons.loading
                        ? "Loading schedule…"
                        : activity.crons.error
                          ? "Schedule unavailable"
                          : dreamCron?.enabled
                            ? `Next scheduled run: ${dreamCron.next_run ? new Date(dreamCron.next_run).toLocaleString() : "not yet determined"}`
                            : "No automatic run scheduled"}
                      {activity.crons.error && (
                        <details>
                          <summary>Schedule error</summary>
                          <InlineErrorNotice error={activity.crons.error} operation="loading schedules" />
                        </details>
                      )}
                    </div>
                  )}
                  {loading && (
                    <p role="status" className={muted}>
                      Loading worker history…
                    </p>
                  )}
                  {!loading && history.length === 0 && (
                    <p className={muted}>
                      {error
                        ? "History could not be loaded."
                        : "No recorded runs. Future runs will appear here."}
                    </p>
                  )}
                  {history.map((worker) => (
                    <WorkerHistory
                      key={worker.worker_id}
                      worker={worker}
                      label={taskLabel(worker)}
                      onOpen={() => setLogWorker(worker)}
                      onKill={() => void stopWorker(worker.worker_id)}
                      onNavigateToItem={onNavigateToItem}
                    />
                  ))}
                  {actionError && (
                    <details open className="text-xs text-red-400">
                      <summary>Could not stop worker</summary>
                      <InlineErrorNotice error={actionError} operation="stopping the worker" />
                    </details>
                  )}
                </section>
              )}
            </section>
          ) : (
            <MemoryActivityServices
              config={config}
              activity={activity}
              nodes={nodes}
              onNavigateToItem={onNavigateToItem}
            />
          )}
        </div>
      </div>
      {spawnOpen && (
        <MemorySpawnWorkerDialog
          onSpawn={async (request) => {
            await spawn(request);
          }}
          onClose={() => setSpawnOpen(false)}
        />
      )}
      {logWorker &&
        (logWorker.session_id ? (
          <RunChatPopup
            title={taskLabel(logWorker)}
            subtitle={[logWorker.agent_provider, logWorker.model].filter(Boolean).join(" / ")}
            status={logWorker.status}
            report={logWorker.report}
            provider={logWorker.agent_provider ?? defaultSpawnProviderId()}
            cwd={logWorker.cwd ?? ""}
            sessionId={logWorker.session_id}
            live={!isTerminalWorkerStatus(logWorker.status)}
            onClose={() => setLogWorker(null)}
          />
        ) : (
          <AgentRunLogPopup worker={logWorker} api={api} onClose={() => setLogWorker(null)} />
        ))}
    </div>
  );
}

export function WorkerHistory({
  worker,
  label,
  onOpen,
  onKill,
  onNavigateToItem,
}: {
  worker: GraphWorkerInfo;
  label: string;
  onOpen: () => void;
  onKill: () => void;
  onNavigateToItem: (id: string) => void;
}) {
  const reason = workerFailureReason(worker);
  return (
    <details className="border-t border-[var(--ct-border)] pt-3 text-xs">
      <summary className="cursor-pointer">
        <span className="font-medium">{label}</span>
        <span className="ml-3 text-[var(--ct-muted-fg)]">
          {workerOutcome(worker)} · {new Date(worker.spawned_at).toLocaleString()}
        </span>
      </summary>
      <div className="pt-3 space-y-3">
        <p className={muted}>
          {[worker.agent_provider, worker.model].filter(Boolean).join(" / ") || "Executor unknown"}{" "}
          · {workerTimeLabels(worker, Date.now()).duration}
        </p>
        {worker.progress_text && <p>{worker.progress_text}</p>}
        {reason && <p className="text-red-400 whitespace-pre-wrap break-words">{reason}</p>}
        {worker.finalization &&
          worker.finalization.state !== "succeeded" &&
          "reason" in worker.finalization && (
            <p className="text-red-400">{worker.finalization.reason}</p>
          )}
        {worker.report && worker.report !== reason && (
          <p className="whitespace-pre-wrap break-words">{worker.report}</p>
        )}
        {worker.extraction_retry && (
          <p>
            {worker.extraction_retry.parked
              ? `Parked after ${worker.extraction_retry.attempt} attempts. Use mem extract-pending to retry.`
              : worker.extraction_retry.retry_after
                ? `Retry eligible ${new Date(worker.extraction_retry.retry_after).toLocaleString()}`
                : `Attempt ${worker.extraction_retry.attempt}`}
          </p>
        )}
        {worker.touched_nodes.length > 0 && (
          <div className="flex flex-wrap gap-2">
            {worker.touched_nodes.map((id) => (
              <button
                key={id}
                type="button"
                className={activityButton}
                onClick={() => onNavigateToItem(id)}
              >
                Open affected memory
              </button>
            ))}
          </div>
        )}
        <div className="flex gap-2">
          <button type="button" className={activityButton} onClick={onOpen}>
            View output
          </button>
          {!isTerminalWorkerStatus(worker.status) && (
            <button type="button" className={activityButton} onClick={onKill}>
              Stop worker
            </button>
          )}
        </div>
        <details className={muted}>
          <summary className="cursor-pointer">Technical details</summary>
          <dl className="mt-2 break-all">
            <dt>Intent</dt>
            <dd>{worker.intent}</dd>
            <dt>Worker</dt>
            <dd>{worker.worker_id}</dd>
            <dt>Started by</dt>
            <dd>{worker.spawned_by}</dd>
          </dl>
        </details>
      </div>
    </details>
  );
}

export const MemoryActivityTab = memo(MemoryActivityTabImpl);
