import { Clock3, Workflow } from "lucide-react";
import { ChatBlockFrame, TONE_COLOR, type ChatBlockAction } from "./ChatBlockFrame";
import { useChatBlockHost, type JobBlockHost } from "./ChatBlockHost";
import { useLiveBinding } from "./liveBinding";
import { BlockNotice } from "./BlockNotice";
import { JOB_KIND_LABEL, JOB_STATE_TONE, jobActionPlan, jobScheduleLine, type JobBlockInfo, type JobBlockState } from "./jobBlockModel";

const LOADING: JobBlockState = { kind: "loading" };

/** `@ds-job <id>`: a live Cron or Blueprint card; Run now, Pause/Resume and Stop run only from the user's click. */
export function JobBlock({ jobRef, source }: { jobRef: string; source: string }) {
  const host = useChatBlockHost().jobs;
  const state = useLiveBinding(host?.binding, jobRef, LOADING);
  const notice = (text: string, missing = false) => <BlockNotice name="job" source={source} label={`Job ${jobRef}`} text={text} icon={Clock3} missing={missing} />;
  if (!host) return notice("Job cards are unavailable in this view.");
  switch (state.kind) {
    case "loading":
      return notice("Reading your jobs…");
    case "unavailable":
      return notice(`Jobs unavailable: ${state.message}`);
    case "missing":
      return notice(`No Cron or Blueprint matches “${jobRef}”.`, true);
    case "ready":
      return <JobCard job={state.job} host={host} jobRef={jobRef} source={source} />;
  }
}

function JobCard({ job, host, jobRef, source }: { job: JobBlockInfo; host: JobBlockHost; jobRef: string; source: string }) {
  const plan = jobActionPlan(job);
  const kind = JOB_KIND_LABEL[job.kind];
  const actions: ChatBlockAction[] = [];
  if (plan.runNow) {
    actions.push({
      id: "run",
      label: "Run now",
      busyLabel: "Starting…",
      primary: true,
      run: async () => {
        await host.runNow(jobRef);
        return `Started ${job.name}.`;
      },
    });
  }
  if (plan.pause) {
    const pausing = plan.pause === "pause";
    actions.push({
      id: plan.pause,
      label: pausing ? "Pause" : "Resume",
      busyLabel: pausing ? "Pausing…" : "Resuming…",
      confirm: pausing
        ? { message: `Pause ${job.name}? It stops firing on schedule until you resume it.`, confirmLabel: "Pause" }
        : { message: `Resume ${job.name}? It fires on its schedule again.`, confirmLabel: "Resume" },
      run: async () => {
        await host.setPaused(jobRef, pausing);
        return pausing ? `Paused ${job.name}.` : `Resumed ${job.name}.`;
      },
    });
  }
  if (plan.stop) {
    actions.push({
      id: "stop",
      label: "Stop",
      busyLabel: "Stopping…",
      confirm: { message: `Stop the running ${kind}? Work in progress ends now.`, confirmLabel: "Stop" },
      run: async () => {
        await host.stop(jobRef);
        return `Stopped ${job.name}.`;
      },
    });
  }
  return (
    <ChatBlockFrame
      name="job"
      source={source}
      title={job.name}
      icon={job.kind === "cron" ? <Clock3 size={14} aria-hidden="true" /> : <Workflow size={14} aria-hidden="true" />}
      status={{ label: job.stateLabel, tone: JOB_STATE_TONE[job.state] }}
      summary={jobScheduleLine(job)}
      body={job.lastResult ? (
        <p data-testid="job-last-result" className="m-0 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
          Last run: <span style={{ color: job.lastResult.tone === "muted" ? "var(--ct-fg)" : TONE_COLOR[job.lastResult.tone] }}>{job.lastResult.label}</span>
        </p>
      ) : undefined}
      details={(
        <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
          <dt>Type</dt><dd className="m-0">{kind}</dd>
          <dt>Schedule</dt><dd className="m-0">{job.schedule}</dd>
          <dt>Id</dt><dd className="m-0 break-all font-mono">{job.id}</dd>
        </dl>
      )}
      actions={actions}
      fullPage={{ label: "Open in Jobs", open: () => host.open(jobRef) }}
      askText={`About the ${kind} ${job.name} (\`${source}\`): `}
      muted={job.state === "paused" || job.state === "stopped"}
    />
  );
}
