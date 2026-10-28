import { useEffect, useMemo, useState, type CSSProperties, type ReactNode } from "react";
import { Bot, Clock3, Send } from "lucide-react";
import type { CronJob, CronRunInfo } from "../../lib/types";
import { describeSchedule, formatFuture, formatTime } from "../../lib/formatters";
import { AgentIcon } from "../AgentIcon";
import { RunChatPopup } from "../AgentRun/RunChatPopup";
import { CronRunHistory } from "../MeshPanel/CronDrawerSection/CronRunHistory";
import { runStatusLabel, runTone } from "../MeshPanel/CronDrawerSection/parts";
import { createMeshResourceActionAdapter, type MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import type { Service } from "../MeshPanel/types";

const defaultAdapter = createMeshResourceActionAdapter();

/** Blueprint node states the shared node styles know, from a cron run's status. */
function nodeState(status: string | undefined): string | undefined {
  if (status === "Running") return "running";
  if (status === "Failed") return "failed";
  if (status === "AwaitingOwnerPrompt") return "held";
  return undefined;
}

function FlowNode({ type, icon, title, subtitle, status, tone, state, body }: {
  type: "trigger" | "agent" | "result"; icon: ReactNode; title: string; subtitle: string; status: string; tone: string; state?: string; body?: string;
}) {
  return <div className="bp-node" data-type={type} data-state={state} role="group" aria-label={`${title}: ${subtitle}, ${status}`}>
    <div className="bp-node__header">
      <span className="bp-node__icon" aria-hidden="true">{icon}</span>
      <span className="bp-node__titles"><span className="bp-node__title">{title}</span><span className="bp-node__subtitle" title={subtitle}>{subtitle}</span></span>
    </div>
    <div className="bp-node__status"><span className="bp-node__dot" style={{ background: tone }} aria-hidden="true" /><span className="bp-node__status-text">{status}</span></div>
    {body && <div className="bp-node__body" data-kind="prompt"><span className="bp-node__clamp" style={{ WebkitLineClamp: 4 } as CSSProperties}>{body}</span></div>}
  </div>;
}

/** A cron as its own small graph: when it fires, which agent runs what, and where the result goes, with its recent runs. */
export function CronFlowView({ job, service, target, actionAdapter = defaultAdapter }: {
  job: CronJob; service: Service; target: string; actionAdapter?: MeshResourceActionAdapter;
}) {
  const [runs, setRuns] = useState<CronRunInfo[]>([]);
  const [openRun, setOpenRun] = useState<CronRunInfo | null>(null);
  const lastRunKey = `${job.last_run?.run_id ?? ""}:${job.last_run?.status ?? ""}`;
  useEffect(() => {
    let live = true;
    void actionAdapter.history(service).then((history) => { if (live) setRuns(Array.isArray(history) ? history.slice().reverse() : []); }, () => { if (live) setRuns([]); });
    return () => { live = false; };
  }, [actionAdapter, service, lastRunKey]);
  const last = job.last_run ?? null;
  const curator = job.role === "curator";
  const agent = curator ? "Curator" : job.model ? `${job.agent} · ${job.model}` : job.agent;
  const schedule = useMemo(() => describeSchedule(job.schedule), [job.schedule]);
  return <section aria-label="Cron flow" className="bp-scope cron-flow mx-auto w-full max-w-4xl px-4 py-5 sm:px-6 sm:py-6">
    <ol className="cron-flow__chain" aria-label="What this cron does">
      <li className="cron-flow__step"><FlowNode type="trigger" icon={<Clock3 size={14} />} title="Schedule" subtitle={schedule}
        status={!job.enabled ? "Paused" : job.next_run ? `Next ${formatFuture(job.next_run)}` : "No upcoming run"} tone={job.enabled ? "var(--ct-accent)" : "var(--ct-yellow)"}
        state={job.enabled ? undefined : "paused"} /></li>
      <li className="cron-flow__step"><FlowNode type="agent" icon={!curator && job.agent ? <AgentIcon providerId={job.agent} className="text-[14px]" /> : <Bot size={14} />}
        title={curator ? "Curator round" : "Agent task"} subtitle={agent} body={curator ? undefined : job.task}
        status={last ? `${runStatusLabel(last.status)} · ${formatTime(last.started_at)}` : "No runs yet"} tone={last ? runTone(last.status) : "var(--ct-muted-fg)"} state={nodeState(last?.status)} /></li>
      <li className="cron-flow__step"><FlowNode type="result" icon={<Send size={14} />} title="Report" subtitle={target}
        status={job.report_target_tab_id ? "Delivered to that tab" : "Kept with the run"} tone="var(--ct-muted-fg)" /></li>
    </ol>
    <div className="mt-5"><CronRunHistory runs={runs} allowed onOpen={setOpenRun} runLabel={curator ? "Curator round" : undefined} /></div>
    {openRun && openRun.session_id && <RunChatPopup title={`cron:${job.name}`} subtitle={formatTime(openRun.started_at)} status={openRun.status} provider={curator ? "" : job.agent}
      cwd={openRun.cwd ?? ""} sessionId={openRun.session_id} live={openRun.status === "Running" || openRun.status === "AwaitingOwnerPrompt"} onClose={() => setOpenRun(null)} />}
  </section>;
}
