import { useEffect, useState, type MouseEvent, type ReactNode } from "react";
import { ChevronUp, CircleCheck, Clock3, Infinity as InfinityIcon, Map, Plus, Timer, Workflow, X } from "lucide-react";
import type { AutomationGraphAdapter } from "../Automation/types";
import { JobConfirmationDialog } from "../Jobs/JobConfirmationDialog";
import { deleteJobConfirmation, deleteMergedJob, stopMergedJob, type JobsActionAdapter } from "../Jobs/jobActions";
import { canStopJob, jobStateLabel, tabJobKey, type MergedJob, type TabJob } from "../Jobs/jobsModel";
import { requestOpenJob } from "../Jobs/openJobEvent";
import { openTabJob, useTabJobs } from "../Jobs/useTabJobs";
import { formatScheduledTime, type DelayedMessage } from "./delayedSend";
import { isSystemDelayedMessage } from "./systemHeartbeatProjection";
import { useResolvedTabActivity, useTabHasPlan } from "./useTabPlanActivity";
import { usePhoneViewport } from "../../hooks/useSidebarViewport";

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
const KIND_SHORT_LABEL = { blueprints: "Blueprints", crons: "Crons", delayed: "Delayed" } as const;

const keepComposerFocus = (event: MouseEvent) => event.preventDefault();

function ContextBubble({ tone, label, onOpen, dismissLabel, dismissTitle, onDismiss, children }: {
  tone: string;
  label?: string;
  onOpen: () => void;
  dismissLabel?: string;
  dismissTitle?: string;
  onDismiss?: () => void;
  children: ReactNode;
}) {
  return (
    <span className="ct-ctx-bubble" data-tone={tone}>
      <button type="button" className="ct-ctx-bubble__open" aria-label={label} onMouseDown={keepComposerFocus} onClick={onOpen}>{children}</button>
      {onDismiss && (
        <button type="button" className="ct-ctx-bubble__dismiss" aria-label={dismissLabel} title={dismissTitle} onMouseDown={keepComposerFocus} onClick={onDismiss}>
          <X size={11} aria-hidden="true" />
        </button>
      )}
    </span>
  );
}

function StateBody({ title, state }: { title: string; state: string }) {
  return (
    <>
      <span className="ct-ctx-bubble__dot" aria-hidden="true" />
      <span className="ct-ctx-bubble__title">{title}</span>
      <span className="ct-ctx-bubble__meta">{jobStateLabel(state)}</span>
    </>
  );
}

function stateTone(state: string): string {
  const label = jobStateLabel(state);
  if (label === "active") return "active";
  if (label === "waiting on you") return "waiting";
  if (label === "stopped") return "stopped";
  return "scheduled";
}

function delayedWhen(message: DelayedMessage): string {
  if (message.state === "stashed") return "stashed";
  return message.state === "failed" ? "failed" : formatScheduledTime(message.scheduledAtMs);
}

function delayedTone(message: DelayedMessage): string {
  if (message.state === "failed") return "waiting";
  return message.state === "stashed" ? "stashed" : "scheduled";
}

type ResolvedPlan = ReturnType<typeof useResolvedTabActivity>;

export function ComposerContextHeader(props: ComposerContextHeaderProps) {
  return useTabHasPlan(props.tabId) ? <PlannedContextHeader {...props} /> : <ContextHeaderBody {...props} plan={null} />;
}

function PlannedContextHeader(props: ComposerContextHeaderProps) {
  const plan = useResolvedTabActivity(props.tabId, props.workspaceId);
  return <ContextHeaderBody {...props} plan={plan.activity ? plan : null} />;
}

function ContextHeaderBody({ tabId, workspaceId, delayed, onEditDelayed, onDeleteDelayed, onScheduleDelayed, draftActive = false, adapter, jobActionAdapter, plan }: ComposerContextHeaderProps & { plan: ResolvedPlan | null }) {
  const { chips, byKey, blueprints, context, refresh } = useTabJobs(tabId, workspaceId, adapter, jobActionAdapter);
  const [requested, setRequested] = useState<ContextKind | null>(null);
  const [shown, setShown] = useState<ContextKind | null>(null);
  const [confirming, setConfirming] = useState<MergedJob | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const phone = usePhoneViewport();
  const [unfolded, setUnfolded] = useState(false);

  useEffect(() => {
    if (!draftActive) return;
    setRequested(null);
    setUnfolded(false);
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

  if (!plan && kinds.length === 0) return null;

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
      <ContextBubble
        key={tabJobKey(chip)}
        tone={stateTone(chip.state)}
        onOpen={() => openTabJob(job, workspaceId ? { workspaceId, tabId } : undefined)}
        dismissLabel={stoppable ? `Stop ${chip.title}` : `Delete ${chip.title}`}
        dismissTitle={stoppable ? "Stop" : "Delete"}
        onDismiss={() => {
          setError(null);
          if (stoppable) void stopMergedJob(job, context).catch(() => setError(`Could not stop ${chip.title}.`));
          else setConfirming(job);
        }}
      >
        <StateBody title={chip.title} state={chip.state} />
      </ContextBubble>
    );
  };

  const blueprintBubble = ({ workspaceId: ownerWorkspace, blueprint, pipeline }: OwnedBlueprint) => (
    <ContextBubble
      key={`blueprint:${blueprint.id}`}
      tone={stateTone(blueprint.state)}
      onOpen={() => requestOpenJob({ workspaceId: ownerWorkspace, target: { teamId: blueprint.owner?.team_id ?? null, blueprintId: pipeline.id }, returnTo: workspaceId ? { workspaceId, tabId } : undefined })}
    >
      <StateBody title={blueprint.name} state={blueprint.state} />
    </ContextBubble>
  );

  const strip: Record<ContextKind, () => ReactNode> = {
    blueprints: () => <>{blueprints.map(blueprintBubble)}{blueprintChips.map(jobBubble)}</>,
    crons: () => <>{[...cronChips].sort((a, b) => Number(b.state === "waiting on you") - Number(a.state === "waiting on you")).map(jobBubble)}</>,
    delayed: () => (
      <>
        {[...delayed].sort((a, b) => Number(a.state === "stashed") - Number(b.state === "stashed")).map((message) => {
          const system = isSystemDelayedMessage(message);
          return (
          <ContextBubble
            key={message.id}
            tone={delayedTone(message)}
            label={system ? `System heartbeat: ${message.text}` : `Edit ${message.state === "stashed" ? "stashed" : "delayed"} message: ${message.text}`}
            onOpen={() => onEditDelayed(message)}
            dismissLabel={system ? undefined : "Cancel delayed message"}
            dismissTitle={system ? undefined : "Cancel"}
            onDismiss={system ? undefined : () => onDeleteDelayed(message.id)}
          >
            {message.state === "stashed"
              ? <InfinityIcon size={13} aria-label="Stashed, sends only when you schedule it" className="ct-ctx-bubble__stash" />
              : <span className="ct-ctx-bubble__meta">{delayedWhen(message)}</span>}
            <span className="ct-ctx-bubble__title ct-ctx-bubble__title--text">{message.text}</span>
          </ContextBubble>
          );
        })}
        <button type="button" className="ct-ctx-bubble ct-ctx-bubble--add" onMouseDown={keepComposerFocus} onClick={onScheduleDelayed}>
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
    const next = delayed.filter((message) => message.state !== "stashed").sort((a, b) => a.scheduledAtMs - b.scheduledAtMs)[0];
    return next ? <ContextTitle kind="delayed" prefix="Next:" primary={delayedWhen(next)} secondary={next.text} /> : null;
  };

  const total = kinds.reduce((sum, kind) => sum + counts[kind], 0);
  const PillIcon = plan ? Map : KIND_ICON[kinds[0]];
  const pill = phone && (
    <button
      type="button"
      className="ct-ctx-pill"
      data-testid="composer-context-pill"
      aria-expanded={unfolded}
      aria-label={unfolded ? "Hide plan and linked items" : "Show plan and linked items"}
      onMouseDown={keepComposerFocus}
      onClick={() => {
        setUnfolded(!unfolded);
        if (unfolded) setRequested(null);
      }}
    >
      <PillIcon size={12} aria-hidden="true" className="ct-ctx-pill__icon" data-kind={plan ? "plan" : kinds[0]} />
      <span className="ct-ctx-pill__title">{plan?.activity?.planTitle ?? KIND_LABEL[kinds[0]]}</span>
      {total > 0 && <span className="ct-ctx-pill__count">+{total}</span>}
      <ChevronUp size={12} aria-hidden="true" className="ct-ctx-pill__chev" />
      {Object.values(alerts).some(Boolean) && <span className="ct-ctx-kind__alert" aria-hidden="true" />}
    </button>
  );

  const body = (
    <>
      <div className="ct-ctx__head">
        {plan ? <PlanTitle plan={plan} /> : fallbackTitle()}
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
                  onMouseDown={keepComposerFocus}
                  onClick={() => toggle(kind)}
                >
                  <Icon size={13} aria-hidden="true" />
                  {phone && <span className="ct-ctx-kind__label">{KIND_SHORT_LABEL[kind]}</span>}
                  {(phone || counts[kind] > 1) && <span className="ct-ctx-kind__count">{counts[kind]}</span>}
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
    </>
  );

  return (
    <div className="ct-ctx" data-testid="composer-context-header" data-open-kind={open ?? undefined} data-phone={phone || undefined}>
      {pill}
      {phone ? (
        <div className="ct-ctx__reveal" data-open={unfolded || undefined}>
          <div className="ct-ctx__reveal-inner">{body}</div>
        </div>
      ) : body}
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
    <button type="button" className="ct-ctx__title" data-kind={kind} title={`${prefix} ${primary}`} onMouseDown={keepComposerFocus} onClick={onOpen}>{body}</button>
  ) : (
    <span className="ct-ctx__title" data-kind={kind} title={`${prefix} ${primary}`}>{body}</span>
  );
}

function PlanTitle({ plan: { activity, onOpen } }: { plan: ResolvedPlan }) {
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
