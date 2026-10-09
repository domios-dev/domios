import { useEffect, useState, type ReactNode } from "react";
import { CircleCheck, Clock3, Map, Plus, Timer, Workflow, X } from "lucide-react";
import type { AutomationGraphAdapter } from "../Automation/types";
import { JobConfirmationDialog } from "../Jobs/JobConfirmationDialog";
import { deleteJobConfirmation, deleteMergedJob, stopMergedJob, type JobsActionAdapter } from "../Jobs/jobActions";
import { canStopJob, jobStateLabel, tabJobKey, type MergedJob, type TabJob } from "../Jobs/jobsModel";
import { requestOpenJob } from "../Jobs/openJobEvent";
import { openTabJob, useTabJobs } from "../Jobs/useTabJobs";
import { formatRelative, type DelayedMessage } from "./delayedSend";
import { useResolvedTabActivity, useTabHasPlan } from "./useTabPlanActivity";

export type ContextKind = "blueprints" | "crons" | "delayed";

interface ComposerContextHeaderProps {
  tabId: string;
  workspaceId: string | null;
  delayed: readonly DelayedMessage[];
  onEditDelayed: (message: DelayedMessage) => void;
  onDeleteDelayed: (id: string) => void;
  onScheduleDelayed: () => void;
  /** True once the composer holds a draft; typing folds any open strip away. */
  draftActive?: boolean;
  adapter?: AutomationGraphAdapter;
  jobActionAdapter?: JobsActionAdapter;
}

type OwnedBlueprint = ReturnType<typeof useTabJobs>["blueprints"][number];

const KIND_ICON = { blueprints: Workflow, crons: Clock3, delayed: Timer } as const;
const KIND_LABEL = { blueprints: "Blueprints", crons: "Crons", delayed: "Delayed messages" } as const;

function stateTone(state: string): string {
  const label = jobStateLabel(state);
  if (label === "active") return "active";
  if (label === "waiting on you") return "waiting";
  if (label === "stopped") return "stopped";
  return "scheduled";
}

function delayedWhen(message: DelayedMessage): string {
  return message.state === "failed" ? "failed" : formatRelative(message.scheduledAtMs);
}

export function ComposerContextHeader({ tabId, workspaceId, delayed, onEditDelayed, onDeleteDelayed, onScheduleDelayed, draftActive = false, adapter, jobActionAdapter }: ComposerContextHeaderProps) {
  const { chips, byKey, blueprints, context, refresh } = useTabJobs(tabId, workspaceId, adapter, jobActionAdapter);
  const hasPlan = useTabHasPlan(tabId);
  const [requested, setRequested] = useState<ContextKind | null>(null);
  const [shown, setShown] = useState<ContextKind | null>(null);
  const [confirming, setConfirming] = useState<MergedJob | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (draftActive) setRequested(null);
  }, [draftActive]);

  const blueprintChips = chips.filter((chip) => chip.kind !== "cron");
  const cronChips = chips.filter((chip) => chip.kind === "cron");
  const counts: Record<ContextKind, number> = {
    blueprints: blueprintChips.length + blueprints.length,
    crons: cronChips.length,
    delayed: delayed.length,
  };
  const alerts: Record<ContextKind, boolean> = {
    blueprints: blueprintChips.some((chip) => chip.state === "waiting on you"),
    crons: cronChips.some((chip) => chip.state === "waiting on you"),
    delayed: delayed.some((message) => message.state === "failed"),
  };
  const kinds = (Object.keys(counts) as ContextKind[]).filter((kind) => counts[kind] > 0);
  const open = requested && counts[requested] > 0 ? requested : null;

  if (!hasPlan && kinds.length === 0) return null;

  const toggle = (kind: ContextKind) => {
    setRequested(open === kind ? null : kind);
    setShown(kind);
  };

  const confirmDelete = async () => {
    if (!confirming) return;
    setBusy(true);
    setError(null);
    try {
      await deleteMergedJob(confirming, context);
    } catch {
      setBusy(false);
      setError(deleteJobConfirmation(confirming).error);
      return;
    }
    await refresh();
    setBusy(false);
    setConfirming(null);
  };

  const jobBubble = (chip: TabJob) => {
    const job = byKey.get(tabJobKey(chip));
    if (!job) return null;
    const stoppable = canStopJob(job);
    return (
      <span key={tabJobKey(chip)} className="ct-ctx-bubble" data-tone={stateTone(chip.state)}>
        <button type="button" className="ct-ctx-bubble__open" onMouseDown={(event) => event.preventDefault()} onClick={() => openTabJob(job, workspaceId ? { workspaceId, tabId } : undefined)}>
          <span className="ct-ctx-bubble__dot" aria-hidden="true" />
          <span className="ct-ctx-bubble__title">{chip.title}</span>
          <span className="ct-ctx-bubble__meta">{jobStateLabel(chip.state)}</span>
        </button>
        <button
          type="button"
          className="ct-ctx-bubble__dismiss"
          aria-label={stoppable ? `Stop ${chip.title}` : `Delete ${chip.title}`}
          title={stoppable ? "Stop" : "Delete"}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => {
            setError(null);
            if (stoppable) void stopMergedJob(job, context).catch(() => setError(`Could not stop ${chip.title}.`));
            else setConfirming(job);
          }}
        >
          <X size={11} aria-hidden="true" />
        </button>
      </span>
    );
  };

  const blueprintBubble = ({ workspaceId: ownerWorkspace, blueprint, pipeline }: OwnedBlueprint) => (
    <span key={`blueprint:${blueprint.id}`} className="ct-ctx-bubble" data-tone={stateTone(blueprint.state)}>
      <button
        type="button"
        className="ct-ctx-bubble__open"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => requestOpenJob({ workspaceId: ownerWorkspace, target: { teamId: blueprint.owner?.team_id ?? null, blueprintId: pipeline.id }, returnTo: workspaceId ? { workspaceId, tabId } : undefined })}
      >
        <span className="ct-ctx-bubble__dot" aria-hidden="true" />
        <span className="ct-ctx-bubble__title">{blueprint.name}</span>
        <span className="ct-ctx-bubble__meta">{jobStateLabel(blueprint.state)}</span>
      </button>
    </span>
  );

  const strip: Record<ContextKind, () => ReactNode> = {
    blueprints: () => <>{blueprints.map(blueprintBubble)}{blueprintChips.map(jobBubble)}</>,
    crons: () => <>{[...cronChips].sort((a, b) => Number(b.state === "waiting on you") - Number(a.state === "waiting on you")).map(jobBubble)}</>,
    delayed: () => (
      <>
        {delayed.map((message) => (
          <span key={message.id} className="ct-ctx-bubble" data-tone={message.state === "failed" ? "waiting" : "scheduled"}>
            <button type="button" className="ct-ctx-bubble__open" aria-label={`Edit delayed message: ${message.text}`} onMouseDown={(event) => event.preventDefault()} onClick={() => onEditDelayed(message)}>
              <span className="ct-ctx-bubble__meta">{delayedWhen(message)}</span>
              <span className="ct-ctx-bubble__title ct-ctx-bubble__title--text">{message.text}</span>
            </button>
            <button type="button" className="ct-ctx-bubble__dismiss" aria-label="Cancel delayed message" title="Cancel" onMouseDown={(event) => event.preventDefault()} onClick={() => onDeleteDelayed(message.id)}>
              <X size={11} aria-hidden="true" />
            </button>
          </span>
        ))}
        <button type="button" className="ct-ctx-bubble ct-ctx-bubble--add" onMouseDown={(event) => event.preventDefault()} onClick={onScheduleDelayed}>
          <Plus size={11} aria-hidden="true" />Schedule
        </button>
      </>
    ),
  };

  const fallbackTitle = () => {
    const owned = blueprints[0];
    if (owned) return <ContextTitle kind="blueprints" prefix="Blueprint:" primary={owned.blueprint.name} secondary={jobStateLabel(owned.blueprint.state)} />;
    const job = blueprintChips[0] ?? cronChips[0];
    if (job) return <ContextTitle kind={job.kind === "cron" ? "crons" : "blueprints"} prefix={job.kind === "cron" ? "Cron:" : "Blueprint:"} primary={job.title} secondary={jobStateLabel(job.state)} />;
    const next = [...delayed].sort((a, b) => a.scheduledAtMs - b.scheduledAtMs)[0];
    return next ? <ContextTitle kind="delayed" prefix="Next:" primary={delayedWhen(next)} secondary={next.text} /> : null;
  };

  return (
    <div className="ct-ctx" data-testid="composer-context-header" data-open-kind={open ?? undefined}>
      <div className="ct-ctx__head">
        {hasPlan ? <PlanTitle tabId={tabId} workspaceId={workspaceId} /> : fallbackTitle()}
        {kinds.length > 0 && (
          <div className="ct-ctx__kinds" role="group" aria-label="Linked to this tab">
            {kinds.map((kind) => {
              const Icon = KIND_ICON[kind];
              return (
                <button
                  key={kind}
                  type="button"
                  className="ct-ctx-kind"
                  data-kind={kind}
                  data-open={open === kind || undefined}
                  aria-pressed={open === kind}
                  aria-label={`${counts[kind]} ${KIND_LABEL[kind].toLowerCase()}${alerts[kind] ? ", one needs you" : ""}`}
                  title={KIND_LABEL[kind]}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => toggle(kind)}
                >
                  <Icon size={13} aria-hidden="true" />
                  {counts[kind] > 1 && <span className="ct-ctx-kind__count">{counts[kind]}</span>}
                  {alerts[kind] && <span className="ct-ctx-kind__alert" aria-hidden="true" />}
                </button>
              );
            })}
          </div>
        )}
      </div>
      <div className="ct-ctx__reveal" data-open={open !== null || undefined}>
        <div className="ct-ctx__reveal-inner">
          {shown && (
            <div className="ct-ctx__strip" data-kind={shown} role="group" aria-label={KIND_LABEL[shown]}>
              {strip[shown]()}
            </div>
          )}
        </div>
      </div>
      {error && !confirming && <span role="alert" className="ct-ctx__error">{error}</span>}
      {confirming && (() => {
        const copy = deleteJobConfirmation(confirming);
        return (
          <JobConfirmationDialog
            title={copy.title}
            description={copy.description}
            confirmLabel={copy.confirmLabel}
            busy={busy}
            error={error}
            onCancel={() => { if (!busy) { setConfirming(null); setError(null); } }}
            onConfirm={() => void confirmDelete()}
          />
        );
      })()}
    </div>
  );
}

function ContextTitle({ kind, prefix, primary, secondary, extra, onOpen }: {
  kind: "plan" | "task" | ContextKind;
  prefix: string;
  primary: string;
  secondary?: string | null;
  extra?: number;
  onOpen?: () => void;
}) {
  const Icon = kind === "plan" ? Map : kind === "task" ? CircleCheck : KIND_ICON[kind];
  const body = (
    <>
      <Icon size={13} aria-hidden="true" className="ct-ctx__title-icon" />
      <span className="ct-ctx__prefix">{prefix}</span>
      <span className="ct-ctx__primary">{primary}</span>
      {extra ? <span className="ct-ctx__extra">+{extra}</span> : null}
      {secondary ? <span className="ct-ctx__secondary">· {secondary}</span> : null}
    </>
  );
  return onOpen ? (
    <button type="button" className="ct-ctx__title" data-kind={kind} title={`${prefix} ${primary}`} onMouseDown={(event) => event.preventDefault()} onClick={onOpen}>{body}</button>
  ) : (
    <span className="ct-ctx__title" data-kind={kind} title={`${prefix} ${primary}`}>{body}</span>
  );
}

function PlanTitle({ tabId, workspaceId }: { tabId: string; workspaceId: string | null }) {
  const { activity, onOpen } = useResolvedTabActivity(tabId, workspaceId);
  if (!activity) return null;
  if (activity.kind === "working") {
    return <ContextTitle kind="task" prefix="Task:" primary={activity.taskTitle} secondary={`Plan: ${activity.planTitle}`} onOpen={onOpen} />;
  }
  return (
    <ContextTitle
      kind="plan"
      prefix={activity.kind === "planning" ? "Planning:" : "Plan:"}
      primary={activity.planTitle}
      extra={activity.kind === "orchestrating" ? activity.extraPlans : undefined}
      onOpen={onOpen}
    />
  );
}
