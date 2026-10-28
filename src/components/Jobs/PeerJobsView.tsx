import { blueprintMeta } from "../Automation/blueprintMeta";
import { useState } from "react";
import { Clock3, Network, RefreshCw, Workflow } from "lucide-react";
import type { TransportClient } from "../../lib/transportClient";
import type { PairedPeer } from "../../lib/types";
import { Loader } from "../shared/Loader";
import { CRON_ID_PREFIX } from "../MeshPanel/types";
import { BackPill } from "../shared/BackPill";
import { UI_BUTTON } from "../shared/design";
import { BlueprintView } from "../Automation/blueprint/BlueprintView";
import { ServiceDrawer } from "../MeshPanel/ServiceDrawer";
import { JobsFilterBar } from "./JobsFilterBar";
import { JobsMachineFilter } from "./JobsMachineFilter";
import { jobsLayout } from "./jobsLayout";
import { cronMeta, JobRow, sentinelRowText } from "./JobRow";
import { filterMachineJobs, jobsMachineAvailability, peerJobsState } from "./jobsMachineModel";
import { jobStateLabel, mergedJobId, NO_JOBS_FILTER, type JobsFilter } from "./jobsModel";
import { usePeerJobs } from "./usePeerJobs";

export function PeerJobsView({ peerId, peers, onMachineChange, onClose, clientForPeer }: {
  clientForPeer?: (peerId: string) => TransportClient;
  peerId: string;
  peers: readonly PairedPeer[];
  onMachineChange: (machineId: string) => void;
  onClose?: () => void;
}) {
  const peer = peers.find((candidate) => candidate.instance_id === peerId);
  const { data, failed, actionFailed, refresh, refreshAfterAction } = usePeerJobs(peerId, jobsMachineAvailability(peerId, peers) === "online", clientForPeer);
  const [filter, setFilter] = useState<JobsFilter>(NO_JOBS_FILTER);
  const [selectedKey, setSelected] = useState<string | null>(null);
  const selected = data?.jobs.find((job) => `${job.kind}:${mergedJobId(job)}` === selectedKey);
  const layout = jobsLayout(filter, data?.workspaces ?? [], data?.teams ?? []);
  const jobs = filterMachineJobs(data?.jobs ?? [], filter);
  const state = peerJobsState(data, failed, jobs.length);
  const service = selected?.kind === "cron" ? data?.resources.find((resource) => resource.id === `${CRON_ID_PREFIX}${selected.cron.id}`) : null;
  const remoteServices = service ? [{ ...service, source_peer_id: peerId, source_peer: peer?.instance_name }] : [];
  return <section aria-label="Jobs" className="flex h-full min-h-0 flex-col bg-[var(--ct-bg)] text-[var(--ct-fg)]">
    <header className="ct-jobs-header flex flex-wrap items-center gap-3 px-4 py-3 sm:px-6">
      {selected ? <BackPill label="All jobs" onBack={() => setSelected(null)} /> : onClose && <BackPill label="where you were" onBack={onClose} />}
      <h2 className="flex items-center gap-2 font-semibold"><Workflow size={16} aria-hidden="true" />Jobs</h2>
      <JobsMachineFilter value={peerId} peers={peers} onChange={onMachineChange} />
      {!selected && <JobsFilterBar filter={filter} layout={layout} teams={data?.teams ?? []} onChange={setFilter} />}
      <button type="button" className={`${UI_BUTTON.icon} ml-auto`} aria-label="Refresh jobs" onClick={refresh}><RefreshCw size={14} aria-hidden="true" /></button>
    </header>
    <div className="min-h-0 flex-1 overflow-y-auto p-4 sm:px-6">
      {state === "unavailable" && <p role={actionFailed ? "alert" : "status"}>Jobs unavailable on {peer?.instance_name ?? "this machine"}{peer?.online === false ? " · machine offline" : ""}.</p>}
      {state === "loading" && <Loader label="Reading jobs" />}
      {state === "partial" && <p role="status">Some jobs on {peer?.instance_name} are unavailable.</p>}
      {data && !selected && <>
        {state === "empty" && <p role="status" aria-label="No jobs">No jobs</p>}
        <ul aria-label="Jobs" className="mx-auto flex max-w-3xl flex-col gap-2">
          {jobs.map((job) => <JobRow key={`${job.kind}:${mergedJobId(job)}`} jobKey={`${job.kind}:${mergedJobId(job)}`}
            icon={job.kind === "cron" ? <Clock3 size={15} /> : <Network size={15} />} name={job.title}
            meta={job.kind === "cron" ? cronMeta(job.cron) : job.kind === "blueprint" ? blueprintMeta(job.blueprint.pipeline.note || job.blueprint.pipeline.nodes[0]?.summary || "", job.state, job.blueprint.pipeline.deletesAt) : sentinelRowText(job.sentinel, job.title).meta}
            type={job.kind === "cron" ? "Cron" : "Blueprint"} tone={job.kind === "cron" ? "cron" : "blueprint"}
            state={jobStateLabel(job.state)} actionItems={[]} onOpen={() => setSelected(`${job.kind}:${mergedJobId(job)}`)} onTriggerRef={() => {}} error={null} />)}
        </ul>
      </>}
      {selected && <article aria-label={selected.title} className="mx-auto max-w-3xl">
        <h3 className="break-words font-semibold">{selected.title}</h3>
        <p className="ct-meta-muted">{peer?.instance_name} · {jobStateLabel(selected.state)}</p>
        {selected.kind === "blueprint" ? <BlueprintView pipeline={selected.blueprint.pipeline} headingLevel={3} />
          : selected.kind === "sentinel" ? <p className="mt-3 break-words">{sentinelRowText(selected.sentinel, selected.title).meta}</p>
            : service ? <ServiceDrawer state={{ kind: "service", id: service.id, sourcePeerId: peerId }} services={remoteServices} peers={[]} thisMachineName="This machine" onClose={() => setSelected(null)} onActionDone={refreshAfterAction} />
              : <p role="status">Job details unavailable on {peer?.instance_name}.</p>}
      </article>}
    </div>
  </section>;
}
