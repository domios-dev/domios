import { actionVisual } from "./toolVisualModel";
import type { ReactNode } from "react";
import { toolCommand, type ToolEvent } from "@codeterm/chat-core";
import { recognize, type CodetermInvocation } from "../codeterm/recognize";
import { describeAction, isSend, navigationTarget, parseSend, taskActivityMetadata, type ActionDescriptor, type NavigationTarget, type PeerNameResolver, type TabNameResolver } from "../codeterm/templates";
import { CompactMessageContent } from "../codeterm/CompactMessageContent";
import { SentMessageBubble } from "../codeterm/SentMessageBubble";
import { usePluginNameResolver } from "../codeterm/PluginNames";
import type { PluginNameResolver } from "../codeterm/pluginAction";
import { ToolCardActionButton, ToolCardChips, ToolCardFrame } from "./ToolCardFrame";
import type { OpenPlan, ResolvedChatPlanRef } from "../codeterm/LinkedText";
import { ToolOutputContent } from "../ToolOutputContent";
import { TabListCard } from "./TabListCard";

type PlanFocusKind = "plan" | "task" | "section";

export interface CodetermNavigationProps {
  resolveTabName?: TabNameResolver;
  resolvePeerName?: PeerNameResolver;
  onFocusTab?: (tabRef: string) => void;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: PlanFocusKind) => ResolvedChatPlanRef | null;
  onOpenSpec?: (specId: string) => void;
  onOpenMemory?: (memoryId: string) => void;
  onOpenSource?: (sourceId: string) => void;
  fontScale?: number;
}

interface Props extends CodetermNavigationProps {
  event: ToolEvent;
  /** The already-recognized invocation; recognized from the event's command when absent. */
  invocation?: CodetermInvocation;
  expanded: boolean;
  onToggle: () => void;
}

function targetLabel(target: NavigationTarget): string {
  if (target.kind === "tab") return "Tab";
  return target.kind[0].toUpperCase() + target.kind.slice(1);
}


function codetermAction(event: ToolEvent, invocation: CodetermInvocation | undefined, navigation: CodetermNavigationProps, resolvePlugin?: PluginNameResolver) {
  const { resolveTabName, resolvePeerName, onFocusTab, onOpenPlan, onOpenSpec, onOpenMemory, onOpenSource, resolvePlanRef } = navigation;
  const command = toolCommand(event);
  const inv = invocation ?? recognize(command.command, command.argv);
  const sent = inv && isSend(inv) ? parseSend(inv, event.result, resolveTabName, resolvePeerName) : undefined;
  const target = inv ? navigationTarget(inv, event.result, String(event.status)) : null;
  const descriptor: ActionDescriptor = inv ? describeAction(inv, event.result, resolveTabName, resolvePeerName, resolvePlugin, String(event.status)) : { lead: "codeterm", chips: [] };
  const taskMetadata = inv ? taskActivityMetadata(inv, event.result, String(event.status)) : undefined;
  const resolvedTask = target?.kind === "task" && target.planId ? resolvePlanRef?.(target.planId, target.id, "task") : undefined;
  const taskTitle = taskMetadata?.name ?? resolvedTask?.taskTitle;
  const taskKey = taskMetadata?.key ?? resolvedTask?.taskKey;
  const taskStatus = taskMetadata?.status;
  if (taskTitle) descriptor.object = taskTitle;
  if (taskKey && !descriptor.chips.includes(taskKey)) descriptor.chips.unshift(taskKey);
  if (taskStatus && !descriptor.chips.includes(taskStatus)) descriptor.chips.push(taskStatus);
  const canNavigate = target != null && (
    target.kind === "tab" ? Boolean(onFocusTab) :
    target.kind === "plan" || target.kind === "task" || target.kind === "section" || target.kind === "goal"
      ? Boolean(onOpenPlan && target.planId)
      : target.kind === "spec" ? Boolean(onOpenSpec) : target.kind === "memory" ? Boolean(onOpenMemory) : Boolean(onOpenSource)
  );
  const isTabList = inv?.path.join(" ") === "tab list";
  const tabListJson = isTabList && inv?.flags.json === true;
  const activityStatus = String(event.status);
  const sendSucceeded = sent?.status?.ok === true && !["error", "pending", "running", "unresolved", "interrupted", "submit_unconfirmed"].includes(activityStatus);
  const sendPending = sent?.status?.pending === true && !["error", "unresolved", "interrupted"].includes(activityStatus);
  const showResult = Boolean(event.result && (!isTabList || tabListJson) && (!inv || !isSend(inv) || !sendSucceeded));
  const hasBody = Boolean(sent || (isTabList && !tabListJson) || descriptor.fields?.length || descriptor.detail || descriptor.bullets?.length || showResult || canNavigate);
  return { inv, sent, sendSucceeded, sendPending, target, descriptor, taskTitle, canNavigate, isTabList, tabListJson, showResult, hasBody };
}

type CodetermAction = ReturnType<typeof codetermAction>;

function ActionBody({ event, action, navigation, inlineTabAction }: { event: ToolEvent; action: CodetermAction; navigation: CodetermNavigationProps; inlineTabAction: boolean }) {
  const { sent, descriptor, target, canNavigate, isTabList, tabListJson, showResult } = action;
  const { onFocusTab, onOpenPlan, onOpenSpec, onOpenMemory, onOpenSource, resolvePlanRef, fontScale } = navigation;
  return (
    <>
      {sent ? (
        <CompactMessageContent
          text={sent.body}
          onOpenPlan={onOpenPlan}
          resolvePlanRef={resolvePlanRef}
          fontScale={fontScale}
        />
      ) : isTabList && !tabListJson ? (
        <TabListCard event={event} />
      ) : descriptor.fields?.length ? (
        <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
          {descriptor.fields.map((field) => (
            <div key={field.label} className="contents">
              <dt className="font-mono" style={{ color: "var(--ct-muted-fg)" }}>{field.label}</dt>
              <dd className="min-w-0 break-words" style={{ color: "var(--ct-fg)" }}>{field.value}</dd>
            </div>
          ))}
        </dl>
      ) : descriptor.detail ? (
        <div className="break-words text-xs" style={{ color: "var(--ct-muted-fg)" }}>{descriptor.detail}</div>
      ) : null}
      {descriptor.bullets && <ul className="list-disc space-y-0.5 pl-4 text-xs marker:text-[var(--ct-muted-fg)]" style={{ color: "var(--ct-muted-fg)" }}>{descriptor.bullets.map((bullet) => <li key={bullet}>{bullet}</li>)}</ul>}
      {showResult && <div data-testid={tabListJson ? "tab-list-json" : "toolcard-result"}><ToolOutputContent text={event.result!} maxHeight="12rem" /></div>}
      {inlineTabAction && canNavigate && target?.kind === "tab" && <ToolCardActionButton label="Show Tab" onClick={() => onFocusTab!(target.id)} testId="codeterm-focus-tab" />}
      {canNavigate && target && target.kind !== "tab" && (
        <ToolCardActionButton
          primary
          testId="codeterm-show-plan"
          label={`Show ${targetLabel(target)}`}
          onClick={() => {
            if (target.kind === "spec") onOpenSpec!(target.id);
            else if (target.kind === "memory") onOpenMemory!(target.id);
            else if (target.kind === "source") onOpenSource!(target.id);
            else onOpenPlan!(target.planId!, target.kind === "plan" || target.kind === "goal" ? undefined : target.id, target.kind === "goal" ? "plan" : target.kind);
          }}
        />
      )}
    </>
  );
}

/** The card template's body alone, for a segment shown inline under its tree line; `fallback` when the template has nothing. */
export function CodetermActionDetail({ event, invocation, fallback, ...navigation }: CodetermNavigationProps & { event: ToolEvent; invocation?: CodetermInvocation; fallback: ReactNode }) {
  const action = codetermAction(event, invocation, navigation, usePluginNameResolver());
  if (!action.hasBody) return <>{fallback}</>;
  return (
    <div data-testid="codeterm-action-detail" className="grid min-w-0 gap-1">
      <ActionBody event={event} action={action} navigation={navigation} inlineTabAction />
    </div>
  );
}

export function CodetermSentContent({ event, invocation, technicalCommand, ...navigation }: CodetermNavigationProps & {
  event: ToolEvent;
  invocation: CodetermInvocation;
  technicalCommand?: string;
}) {
  const target = navigationTarget(invocation, event.result);
  const resolvedTargetName = target?.kind === "tab" ? navigation.resolveTabName?.(target.id) : undefined;
  return <>
    <SentMessageBubble
      eventId={event.id}
      inv={invocation}
      result={event.result}
      resolveTabName={navigation.resolveTabName}
      resolvePeerName={navigation.resolvePeerName}
      onOpenPlan={navigation.onOpenPlan}
      resolvePlanRef={navigation.resolvePlanRef}
      fontScale={navigation.fontScale}
      onFocusTab={navigation.onFocusTab && target?.kind === "tab" && resolvedTargetName
        ? () => navigation.onFocusTab!(target.id)
        : undefined}
      technicalCommand={technicalCommand}
      activityStatus={event.status}
    />

  </>;
}

export function CodetermActionCard({ event, invocation, expanded, onToggle, ...navigation }: Props) {
  const action = codetermAction(event, invocation, navigation, usePluginNameResolver());
  const { inv, descriptor, target, taskTitle, canNavigate, sendSucceeded, sendPending } = action;
  if (action.sent && inv) {
    const command = toolCommand(event).command;
    return <CodetermSentContent event={event} invocation={inv} technicalCommand={command} {...navigation} />;
  }
  const collapsedChips = descriptor.chips.filter((chip) => !/\b(?:pl|task|sec|sp|spec|mem|src)_[a-z0-9][a-z0-9_-]*/i.test(chip));
  const displayEvent: ToolEvent = {
    ...event,
    status: sendSucceeded ? "success" : sendPending ? "pending" : event.status,
    summary: descriptor.object ?? descriptor.lead,
    detail: undefined,
  };

  return (
    <ToolCardFrame
      event={displayEvent}
      expanded={expanded}
      onToggle={onToggle}
      title={taskTitle ? "" : descriptor.object ? descriptor.lead : ""}
      summary={descriptor.object ?? descriptor.lead}
      variant="codeterm"
      disclosureLabel="full command"
      visualOverride={inv ? actionVisual(inv) : undefined}
      metric={collapsedChips.length ? <ToolCardChips chips={collapsedChips} wrap /> : undefined}
      action={canNavigate && target?.kind === "tab"
        ? <ToolCardActionButton label="Show Tab" onClick={() => navigation.onFocusTab!(target.id)} testId="codeterm-focus-tab" />
        : canNavigate && target?.kind === "task" && !expanded
          ? <ToolCardActionButton label="Show Task" onClick={() => navigation.onOpenPlan!(target.planId!, target.id, "task")} testId="codeterm-show-plan" />
          : undefined}
    >
      <ActionBody event={event} action={action} navigation={navigation} inlineTabAction={false} />
    </ToolCardFrame>
  );
}
