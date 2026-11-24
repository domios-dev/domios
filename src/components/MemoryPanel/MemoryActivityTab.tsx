import { InlineErrorNotice } from "../shared/ErrorNotice";
import { Fragment, memo, useState } from "react";
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
import { Button } from "../shared/Button";
import { UI_BUTTON } from "../shared/design";

interface Props {
  api: ApiConfig;
  config?: AppConfig;
  nodes: Map<string, MemNode>;
  onNavigateToItem: (id: string) => void;
}

const activityButton = `${UI_BUTTON.ghost} ct-btn-sm ct-liquid-interactive gap-2`;
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

  const historyPanel = selected && (
    <section
      id="memory-agent-details"
      aria-label={`${selected} details`}
      className="space-y-2 px-3 pb-3 pt-2.5"
    >
      <h3 className="sr-only">
        {agents.find((agent) => agent.kind === selected)?.title || "Other workers"} history
      </h3>
      {selected === "dreaming" && dreamCron?.last_run && (
        <p className={muted}>
          Last scheduled <strong className="font-[550] text-[var(--ct-fg)]">{dreamCron.last_run.status} ·{" "}
          {new Date(dreamCron.last_run.started_at).toLocaleString()}</strong>
        </p>
      )}
      {selected === "dreaming" && (
        <div className={muted}>
          {activity.crons.loading
            ? "Loading schedule…"
            : activity.crons.error
              ? "Schedule unavailable"
              : dreamCron?.enabled
                ? <>Next <strong className="font-[550] text-[var(--ct-fg)]">{dreamCron.next_run ? new Date(dreamCron.next_run).toLocaleString() : "not yet determined"}</strong></>
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
        <details open className="text-xs text-[var(--ct-red)]">
          <summary>Could not stop worker</summary>
          <InlineErrorNotice error={actionError} operation="stopping the worker" />
        </details>
      )}
      <div className="flex justify-end">
        <Button size="sm" onClick={() => setSelected(null)}>Close details</Button>
      </div>
    </section>
  );

  return (
    <div
      className="absolute inset-0 flex flex-col text-[var(--ct-fg)]"
      data-testid="memory-activity-tab"
    >
      <div className="flex shrink-0 items-center gap-2 px-4 py-3 sm:px-6">
        <div role="tablist" aria-label="Memory activity" className="flex h-7 items-center gap-0.5 rounded-[9px] border border-[var(--ct-border-subtle)] bg-[var(--ct-fg)]/6 p-0.5">
          {(["workers", "services"] as const).map((value) => (
            <button
              key={value}
              type="button"
              role="tab"
              id={`activity-${value}`}
              aria-selected={tab === value}
              aria-controls={`activity-${value}-panel`}
              className="h-full cursor-pointer rounded-md !border !border-transparent px-2.5 text-xs font-semibold text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] aria-selected:!border-[var(--ct-accent)]/40 aria-selected:bg-[var(--ct-accent)]/16 aria-selected:text-[var(--ct-fg)]"
              onClick={() => setTab(value)}
            >
              {value === "workers" ? "Workers" : "Services"}
            </button>
          ))}
        </div>
        <Button
          variant="icon"
          size="sm"
          className="ml-auto"
          aria-label="Refresh activity"
          onClick={() => {
            void refresh();
            setRevision((value) => value + 1);
          }}
        >
          <RefreshCw size={14} aria-hidden="true" />
        </Button>
        {tab === "workers" && (
          <Button size="sm" className="gap-1.5 max-sm:!w-8 max-sm:!px-0" aria-label="New worker" onClick={() => setSpawnOpen(true)}>
            <Plus size={14} aria-hidden="true" />
            <span className="max-sm:hidden">New worker</span>
          </Button>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-6 pt-2 sm:px-6">
        <div className="mx-auto max-w-[800px]">
          {tab === "workers" ? (
            <section
              role="tabpanel"
              id="activity-workers-panel"
              aria-labelledby="activity-workers"
              className="space-y-3.5"
            >
              <div>
                <h2 className="mb-1 text-[13.5px] font-semibold">Memory agents</h2>
                <p className="text-[12.5px] text-[var(--ct-muted-fg)]">
                  Configured here, working in the background. Open an agent to see its runs.
                </p>
              </div>
              {error && (
                <details className="text-xs text-[var(--ct-red)]">
                  <summary className="cursor-pointer">
                    Worker status unavailable. Refresh to try again.
                  </summary>
                  <InlineErrorNotice error={error} operation="loading worker status" />
                </details>
              )}
              <div className="ct-card ct-card--static divide-y divide-[var(--ct-border-subtle)] overflow-hidden">
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
                    "Harness default";
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
                  const statusStyle = current.live.length
                    ? "text-[var(--ct-accent)]"
                    : failed || current.parked.length
                      ? "bg-[var(--ct-red)]/12 text-[var(--ct-red)]"
                      : state === "Last run completed"
                        ? "text-[var(--ct-green)]"
                        : "text-[var(--ct-muted-fg)]";
                  return (
                    <Fragment key={kind}>
                      <button
                        type="button"
                        aria-expanded={selected === kind}
                        aria-controls="memory-agent-details"
                        className="group grid w-full cursor-pointer grid-cols-[32px_minmax(0,1fr)_14px] items-start gap-x-3.5 gap-y-2.5 px-3.5 py-3.5 text-left hover:bg-[var(--ct-bg-hover)] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)] aria-expanded:bg-[var(--ct-bg-hover)] sm:grid-cols-[32px_minmax(0,1fr)_auto_14px] sm:items-center"
                        onClick={() => setSelected(selected === kind ? null : kind)}
                      >
                        <span className="ct-card__icon !size-8 self-start"><Icon size={16} aria-hidden="true" /></span>
                        <span className="flex-1 min-w-0">
                          <span className="block text-[13.5px] font-semibold">{title}</span>
                          <span className="block text-[12.5px] text-[var(--ct-muted-fg)]">{description}</span>
                          <span className={`${muted} mt-1 block break-words`}>
                            {config
                              ? `${provider?.display_name || providerId || "Harness not configured"} · ${modelLabel}`
                              : "Configuration unavailable"}
                          </span>
                          {current.live[0] && (
                            <span className="block text-xs mt-2 truncate">
                              {current.live[0].progress_text || taskLabel(current.live[0])}
                            </span>
                          )}
                        </span>
                        <span
                          className={`col-start-2 row-start-2 inline-flex w-fit items-center gap-1.5 rounded-full px-2 py-0.5 text-xs sm:col-start-3 sm:row-start-1 ${statusStyle}`}
                        >
                          <span className={`size-1.5 shrink-0 rounded-full bg-current ${current.live.length ? "motion-safe:animate-pulse" : ""}`} aria-hidden="true" />
                          {state}
                        </span>
                        <ChevronRight size={14} className="col-start-3 row-start-1 mt-1 shrink-0 text-[var(--ct-muted-fg)] group-aria-expanded:rotate-90 sm:col-start-4" aria-hidden="true" />
                      </button>
                      {selected === kind && historyPanel}
                    </Fragment>
                  );
                })}
                {workers.some((worker) => workerAgentKind(worker) === "custom") && (
                  <button
                    type="button"
                    className="group w-full cursor-pointer p-3.5 text-left text-[13.5px] font-semibold hover:bg-[var(--ct-bg-hover)] aria-expanded:bg-[var(--ct-bg-hover)]"
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
                    <ChevronRight size={14} className="ml-2 inline group-aria-expanded:rotate-90" aria-hidden="true" />
                  </button>
                )}
                {selected === "custom" && historyPanel}
              </div>
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
          <RunChatPopup tabId={logWorker.tab_id}
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
  const outcome = workerOutcome(worker);
  const duration = workerTimeLabels(worker, Date.now()).duration;
  const outcomeTone = workerNeedsAttention(worker)
    ? "text-[var(--ct-red)]"
    : outcome === "Completed"
      ? "text-[var(--ct-green)]"
      : !isTerminalWorkerStatus(worker.status)
        ? "text-[var(--ct-accent)]"
        : "text-[var(--ct-muted-fg)]";
  return (
    <details className="group/run rounded-lg border-t border-[var(--ct-border-subtle)] text-xs">
      <summary className="grid cursor-pointer list-none grid-cols-[auto_minmax(0,1fr)_14px] items-center gap-x-2.5 gap-y-1 rounded-lg px-2.5 py-2 hover:bg-[var(--ct-bg-hover)] sm:grid-cols-[auto_minmax(0,1fr)_auto_auto_14px] [&::-webkit-details-marker]:hidden">
        <span className="col-start-2 row-start-1 truncate text-right text-[var(--ct-muted-fg)] tabular-nums sm:col-start-1 sm:text-left">
          {new Date(worker.spawned_at).toLocaleString()}
        </span>
        <span className="col-span-2 row-start-2 min-w-0 truncate text-[var(--ct-muted-fg)] sm:col-span-1 sm:col-start-2 sm:row-start-1 sm:text-[var(--ct-fg)]">
          {label}<span className="sm:hidden"> · {duration}</span>
        </span>
        <span className={`col-start-1 row-start-1 inline-flex items-center gap-1.5 sm:col-start-3 ${outcomeTone}`}>
          <span className="size-1.5 shrink-0 rounded-full bg-current" aria-hidden="true" />
          {outcome}
        </span>
        <span className="hidden text-[var(--ct-muted-fg)] sm:col-start-4 sm:row-start-1 sm:block">{duration}</span>
        <ChevronRight size={14} className="col-start-3 row-start-1 text-[var(--ct-muted-fg)] group-open/run:rotate-90 sm:col-start-5" aria-hidden="true" />
      </summary>
      <div className="space-y-3 px-2.5 pb-2.5 pt-1">
        <p className={muted}>
          {[worker.agent_provider, worker.model].filter(Boolean).join(" · ") || "Executor unknown"}
        </p>
        {worker.progress_text && <p>{worker.progress_text}</p>}
        {reason && <p className="whitespace-pre-wrap break-words text-[var(--ct-red)]">{reason}</p>}
        {worker.finalization &&
          worker.finalization.state !== "succeeded" &&
          "reason" in worker.finalization && (
            <p className="text-[var(--ct-red)]">{worker.finalization.reason}</p>
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
