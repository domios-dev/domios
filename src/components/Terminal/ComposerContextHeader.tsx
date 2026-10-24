import { SessionGoalDetails } from "./SessionGoalDetails";
import { ComposerContextChip } from "./ComposerContextChip";
import { RemovedChatPlugin } from "./RemovedChatPlugin";
import { removedChatPluginId } from "./removedChatPluginError";
import { SYSTEM_ORIGIN_LABEL, type ChatSessionIntent } from "@codeterm/chat-core";
import { useRuntimeStatusPopover } from "./useRuntimeStatusPopover";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { memo, useEffect, useId, useRef, useState, type MouseEvent, type ReactNode, type RefObject } from "react";
import { Activity, Bot, ChevronUp, CircleCheck, Clock3, HeartPulse, TriangleAlert, Infinity as InfinityIcon, Map, Plus, Timer, Target, Workflow, X } from "lucide-react";
import type { AutomationGraphAdapter } from "../Automation/types";
import { JobConfirmationDialog } from "../Jobs/JobConfirmationDialog";
import { deleteJobConfirmation, deleteMergedJob, stopMergedJob, type JobsActionAdapter } from "../Jobs/jobActions";
import { canStopJob, jobStateLabel, tabJobKey, type MergedJob, type TabJob } from "../Jobs/jobsModel";
import { requestOpenJob } from "../Jobs/openJobEvent";
import { openTabJob, useTabJobs } from "../Jobs/useTabJobs";
import { formatClock, formatScheduledTime, type DelayedMessage } from "./delayedSend";
import { composerDelayedModel } from "./composerDelayedModel";
import { presentSystemReminder } from "./systemReminderModel";
import { SystemHeartbeatDetail } from "./SystemHeartbeatDetail";
import { useResolvedTabActivity, useTabHasPlan } from "./useTabPlanActivity";
import { usePhoneViewport } from "../../hooks/useSidebarViewport";
import { useMuxSelector } from "../../store/multiplexer";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { goalVisible } from "./goalModel";
import { ComposerBackgroundWorkPanel } from "./ComposerBackgroundWorkPanel";
import { useComposerBackgroundWorkStatus } from "./ComposerBackgroundWorkContext";
import { composerBackgroundWorkBubble } from "./composerBackgroundWorkModel";
import { AnchoredPopover } from "../shared/AnchoredPopover";

export type ContextKind = "blueprints" | "crons" | "delayed" | "goal" | "background";

interface ComposerContextHeaderProps {
  pluginError?: string | null;
  sessionIntent?: ChatSessionIntent | null;
  tabId: string;
  workspaceId: string | null;
  delayed: readonly DelayedMessage[];
  heartbeats?: readonly DelayedMessage[];
  onEditDelayed: (message: DelayedMessage) => void;
  onDeleteDelayed: (id: string) => void;
  onScheduleDelayed: () => void;
  /** True once the composer holds a draft; typing folds any open strip away. */
  draftActive?: boolean;
  adapter?: AutomationGraphAdapter;
  jobActionAdapter?: JobsActionAdapter;
}

type OwnedBlueprint = ReturnType<typeof useTabJobs>["blueprints"][number];

const KIND_ICON = { blueprints: Workflow, crons: Clock3, delayed: Timer, goal: Target, background: Activity } as const;
const KIND_LABEL = { blueprints: "Blueprints", crons: "Crons", delayed: "Delayed messages", goal: "Session goal", background: "Background work" } as const;
const KIND_SHORT_LABEL = { blueprints: "Blueprints", crons: "Crons", delayed: "Delayed", goal: "Goal", background: "Background" } as const;
const KIND_NOUN: Record<Exclude<ContextKind, "goal">, [string, string]> = {
  blueprints: ["blueprint", "blueprints"],
  crons: ["cron", "crons"],
  delayed: ["delayed message", "delayed messages"],
  background: ["background work", "background work"],
};

function kindCount(kind: Exclude<ContextKind, "goal">, count: number): string {
  const [one, many] = KIND_NOUN[kind];
  return `${count} ${count === 1 ? one : many}`;
}

const keepComposerFocus = (event: MouseEvent) => event.preventDefault();

function ContextBubble({ tone, label, onOpen, dismissLabel, dismissTitle, onDismiss, triggerRef, title, expanded, children }: {
  triggerRef?: RefObject<HTMLButtonElement | null>;
  title?: string;
  expanded?: boolean;
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
      <button ref={triggerRef} title={title} aria-expanded={expanded} aria-haspopup={expanded === undefined ? undefined : "dialog"} type="button" className="ct-ctx-bubble__open" aria-label={label} onMouseDown={keepComposerFocus} onClick={onOpen}>{children}</button>
      {onDismiss && (
        <button type="button" className="ct-ctx-bubble__dismiss" aria-label={dismissLabel} title={dismissTitle} onMouseDown={keepComposerFocus} onClick={onDismiss}>
          <X size={11} aria-hidden="true" />
        </button>
      )}
    </span>
  );
}

function ContextPopoverBubble({ label, tone, title, children, details }: {
  label: string;
  tone: string;
  title?: string;
  children: ReactNode;
  details: ReactNode;
}) {
  const { open, triggerRef, popoverRef, toggle, close } = useRuntimeStatusPopover();
  return <div className="inline-flex min-w-0 max-w-full">
    <ComposerContextChip kind={tone} open={open} ref={triggerRef} aria-label={label} title={title} aria-expanded={open} aria-haspopup="dialog" onClick={toggle}>{children}</ComposerContextChip>
    <AnchoredPopover open={open} anchor={triggerRef} placement="top-end" onClose={close}>
      <div ref={popoverRef} role="dialog" aria-label={label} tabIndex={-1}
        className="ct-menu w-64 max-w-full whitespace-pre-wrap break-words p-3 text-xs text-[var(--ct-fg)]">{details}</div>
    </AnchoredPopover>
  </div>;
}

function RemovedPluginBubble({ error }: { error: string }) {
  return <ContextPopoverBubble tone="waiting" label="Plugin removed" details={<RemovedChatPlugin error={error} />}>
    <TriangleAlert size={13} aria-hidden="true" />
  </ContextPopoverBubble>;
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

export const ComposerContextHeader = memo(function ComposerContextHeader(props: ComposerContextHeaderProps) {
  return useTabHasPlan(props.tabId) ? <PlannedContextHeader {...props} /> : <ContextHeaderBody {...props} plan={null} />;
});

function PlannedContextHeader(props: ComposerContextHeaderProps) {
  const plan = useResolvedTabActivity(props.tabId, props.workspaceId);
  return <ContextHeaderBody {...props} plan={plan.activity ? plan : null} />;
}

function ContextHeaderBody({ pluginError, sessionIntent, tabId, workspaceId, delayed: queue, heartbeats = [], onEditDelayed, onDeleteDelayed, onScheduleDelayed, draftActive = false, adapter, jobActionAdapter, plan }: ComposerContextHeaderProps & { plan: ResolvedPlan | null }) {
  const delayedModel = composerDelayedModel(queue, Date.now());
  const delayed = delayedModel.chips;
  const [heartbeatId, setHeartbeatId] = useState<string | null>(null);
  const heartbeat = heartbeats.find(message => message.id === heartbeatId);
  const providerId = useMuxSelector(state => {
    const tab = state.tabs[tabId];
    return tab?.agent_type ?? (tab?.backing?.kind === "chat_backend" ? tab.backing.plugin_id : null);
  });
  const descriptor = useAgentDescriptor(providerId);
  const backgroundWork = useComposerBackgroundWorkStatus();
  const backgroundBubble = composerBackgroundWorkBubble(backgroundWork.items);
  const backgroundItems = backgroundBubble?.items ?? [];
  const onlySubagents = backgroundItems.length > 0 && backgroundItems.every(item => item.kind === "agent");
  const internalAgentLabel = `Internal agent of ${descriptor?.display_name ?? "parent harness"}`;
  const { chips, byKey, blueprints, context, refresh } = useTabJobs(tabId, workspaceId, adapter, jobActionAdapter);
  const [requested, setRequested] = useState<ContextKind | null>(null);
  const goalTriggerRef = useRef<HTMLButtonElement>(null);
  const [unavailableGoal, setUnavailableGoal] = useState<string | null>(null);
  const goalKey = JSON.stringify([tabId, providerId, sessionIntent?.goal]);
  const hasGoal = goalVisible(descriptor?.agent_runtime, sessionIntent) && unavailableGoal !== goalKey;
  const [confirming, setConfirming] = useState<MergedJob | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const phone = usePhoneViewport();
  const linkedGroupId = useId();
  const [unfolded, setUnfolded] = useState(false);

  useEffect(() => {
    if (requested !== "goal") return;
    const dismiss = (event: KeyboardEvent) => {
      if (event.code !== "Escape" || event.defaultPrevented) return;
      setRequested(null);
      goalTriggerRef.current?.focus();
    };
    document.addEventListener("keydown", dismiss);
    return () => document.removeEventListener("keydown", dismiss);
  }, [requested]);

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
    delayed: delayedModel.count + heartbeats.length,
    goal: Number(hasGoal),
    background: backgroundBubble?.count ?? 0,
  };
  const alerts: Record<ContextKind, boolean> = {
    blueprints: blueprintChips.some((chip) => chip.state === "waiting on you"),
    crons: cronChips.some((chip) => chip.state === "waiting on you"),
    delayed: delayedModel.alert,
    goal: false,
    background: false,
  };
  const kinds = (Object.keys(counts) as ContextKind[]).filter((kind) => counts[kind] > 0 || (kind === "delayed" && heartbeats.length > 0));
  const open = requested && kinds.includes(requested) ? requested : null;

  const hasPluginError = removedChatPluginId(pluginError ?? null) !== null;
  if (!hasGoal && !hasPluginError && !plan && kinds.length === 0 && heartbeats.length === 0) return null;

  const toggle = (kind: ContextKind) => {
    setRequested(open === kind ? null : kind);
    setUnfolded(true);
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
    goal: () => sessionIntent && <SessionGoalDetails key={goalKey} tabId={tabId} intent={sessionIntent} triggerRef={goalTriggerRef} onUnavailable={() => { setUnavailableGoal(goalKey); setRequested(null); }} />,
    blueprints: () => <>{blueprints.map(blueprintBubble)}{blueprintChips.map(jobBubble)}</>,
    crons: () => <>{[...cronChips].sort((a, b) => Number(b.state === "waiting on you") - Number(a.state === "waiting on you")).map(jobBubble)}</>,
    delayed: () => (
      <>
        {heartbeats.map(message => {
          const reminder = presentSystemReminder(message, Date.now(), formatClock);
          return (
            <ContextBubble key={message.id} tone="heartbeat" label={reminder.accessibleLabel} expanded={heartbeatId === message.id} onOpen={() => setHeartbeatId(message.id)}>
              <HeartPulse size={13} aria-hidden="true" />
              <span className="ct-ctx-bubble__title">{reminder.label}</span>
              <span className="ct-ctx-bubble__meta">{reminder.meta}</span>
            </ContextBubble>
          );
        })}
        {delayed.map((message) => {
          return (
          <ContextBubble
            key={message.id}
            tone={delayedTone(message)}
            label={`Edit ${message.state === "stashed" ? "stashed" : "delayed"} message: ${message.text}`}
            onOpen={() => onEditDelayed(message)}
            dismissLabel="Cancel delayed message"
            dismissTitle="Cancel"
            onDismiss={() => onDeleteDelayed(message.id)}
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
    background: () => <ComposerBackgroundWorkPanel items={backgroundItems} onStop={backgroundWork.stop} internalAgentLabel={internalAgentLabel} />,
  };

  const fallbackTitle = () => {
    const owned = blueprints[0];
    if (owned) return <ContextTitle kind="blueprints" prefix="Blueprint:" primary={owned.blueprint.name} secondary={jobStateLabel(owned.blueprint.state)} />;
    const job = blueprintChips[0] ?? cronChips[0];
    if (job) return <ContextTitle kind={job.kind === "cron" ? "crons" : "blueprints"} prefix={job.kind === "cron" ? "Cron:" : "Blueprint:"} primary={job.title} secondary={jobStateLabel(job.state)} />;
    const next = delayed.filter((message) => message.state !== "stashed").sort((a, b) => a.scheduledAtMs - b.scheduledAtMs)[0];
    return next ? <ContextTitle kind="delayed" prefix="Next:" primary={delayedWhen(next)} secondary={next.text} /> : null;
  };

  const total = kinds.reduce((sum, kind) => sum + counts[kind], 0) + Number(hasPluginError);
  const PillIcon = plan ? Map : kinds.length > 0 ? KIND_ICON[kinds[0]] : heartbeats.length > 0 ? HeartPulse : hasGoal ? Target : TriangleAlert;
  const pill = phone && (
    <button
      type="button"
      className="ct-ctx-pill"
      data-testid="composer-context-pill"
      aria-controls={total > 0 ? linkedGroupId : undefined}
      aria-expanded={unfolded}
      aria-label={unfolded ? "Hide plan and linked items" : "Show plan and linked items"}
      onMouseDown={keepComposerFocus}
      onClick={() => {
        setUnfolded(!unfolded);
        if (unfolded) setRequested(null);
      }}
    >
      <PillIcon size={12} aria-hidden="true" className="ct-ctx-pill__icon" data-kind={plan ? "plan" : kinds[0]} />
      <span className="ct-ctx-pill__title">{plan?.activity?.planTitle ?? KIND_LABEL[kinds[0]] ?? (heartbeats.length > 0 ? SYSTEM_ORIGIN_LABEL : hasGoal ? "Session goal" : hasPluginError ? "Plugin removed" : "Subagents")}</span>
      {total > 0 && <span className="ct-ctx-pill__count">+{total}</span>}
      {total > 0 && <ChevronUp size={12} aria-hidden="true" className="ct-ctx-pill__chev" />}
      {Object.values(alerts).some(Boolean) && <span className="ct-ctx-kind__alert" aria-hidden="true" />}
    </button>
  );

  const body = open && (
    <>
      <div className="ct-ctx__reveal" data-open={open !== null || undefined}>
        <div className="ct-ctx__reveal-inner">
          <div className="ct-ctx__strip" data-kind={open} role={open === "goal" ? "dialog" : "group"} aria-label={KIND_LABEL[open]}
            tabIndex={0} style={{ boxSizing: "border-box", maxHeight: 200, overflowY: "auto", overscrollBehaviorY: "contain", touchAction: "pan-y" }}>
            {strip[open]()}
          </div>
        </div>
      </div>
    </>
  );

  const linkedChips = (
    (hasGoal || hasPluginError || kinds.length > 0 || heartbeats.length > 0) && (
      <div id={linkedGroupId} hidden={phone && !unfolded} className="ct-ctx__kinds" role="group" aria-label="Linked to this tab">
        {hasPluginError && pluginError && <RemovedPluginBubble error={pluginError} />}
        {kinds.map((kind) => {
          const Icon = kind === "background" && onlySubagents ? Bot : KIND_ICON[kind];
          return (
            <ComposerContextChip
              key={kind}
              ref={kind === "goal" ? goalTriggerRef : undefined}
              kind={kind}
              open={open === kind}
              aria-pressed={open === kind}
              aria-label={kind === "goal" ? KIND_LABEL.goal : `${kindCount(kind, counts[kind])}${kind === "delayed" && heartbeats.length > 0 ? ", system reminder" : ""}${alerts[kind] ? ", one needs you" : ""}${kind === "background" && onlySubagents ? `, ${internalAgentLabel}` : ""}`}
              title={kind === "background" && onlySubagents ? `${KIND_LABEL[kind]} · ${internalAgentLabel}` : KIND_LABEL[kind]}
              onClick={() => toggle(kind)}
            >
              <Icon size={13} aria-hidden="true" />
              {!phone && kind === "delayed" && <span className="ct-ctx-kind__label">{KIND_SHORT_LABEL[kind]}</span>}
              {(phone || kind === "delayed" || kind === "background" || counts[kind] > 1) && <span className="ct-ctx-kind__count">{counts[kind]}</span>}
              {alerts[kind] && <span className="ct-ctx-kind__alert" aria-hidden="true" />}
            </ComposerContextChip>
          );
        })}
      </div>
    )
  );

  return (
    <div className="ct-ctx" data-testid="composer-context-header" data-open-kind={open ?? undefined} data-phone={phone || undefined}>
      <div className="ct-ctx__head ct-ctx__compact">
        {phone ? pill : plan ? <PlanTitle plan={plan} /> : fallbackTitle()}
        {!phone && linkedChips}
      </div>
      {phone && linkedChips}
      {heartbeat && <SystemHeartbeatDetail message={heartbeat} busy={false} onClose={() => setHeartbeatId(null)} />}
      {open && (phone ? (
        <div className="ct-ctx__reveal" data-open={unfolded || undefined}>
          <div className="ct-ctx__reveal-inner">{body}</div>
        </div>
      ) : body)}
      {error && !confirming && <InlineErrorNotice error={error} operation="updating the composer context" blocking />}
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
