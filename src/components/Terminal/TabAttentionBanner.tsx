import { RichMarkdown } from "@codeterm/chat-ui";
import { useLayoutEffect, useRef, useState } from "react";
import { ChevronDown, X } from "lucide-react";
import type { NotificationAction, Tab } from "../../lib/types";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import { emitTabUiMode } from "../../lib/chatUiMode";
import { runNotificationAction } from "../../lib/notificationActions";
import { UI_BUTTON } from "../shared/design";
import { attentionActions, tabAttentionLabel, type TabAttention } from "../Sidebar/tabAttention";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { attentionSurfaceLayout } from "../Notifications/attentionSurfaceModel";
import { levelIcon } from "../Notifications/levelIcon";
import { useTabAttention } from "./useTabAttention";

export function TabAttentionBanner({ title, model, onDismiss, onAction, onOpenPrompt }: {
  title: string;
  model: TabAttention | null;
  onDismiss: () => void;
  onAction: (action: NotificationAction) => void;
  onOpenPrompt?: () => void;
}) {
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  const [width, setWidth] = useState(0);
  const host = useRef<HTMLElement>(null);
  const details = useRef<HTMLDivElement>(null);
  const key = model?.notification?.id ?? model?.reason ?? "";
  const expanded = expandedKey === key;
  useLayoutEffect(() => {
    const element = host.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    observer.observe(element);
    return () => observer.disconnect();
  }, [key]);
  useLayoutEffect(() => {
    if (expanded) details.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [expanded]);
  if (!model) return null;
  const notification = model.notification;
  const actions = attentionActions(notification);
  const localActions = notification?.resolved ? [] : notification?.local_actions ?? [];
  const labels = [...actions, ...localActions].map((action) => action.label);
  if (model.kind === "waiting" && onOpenPrompt) labels.push("Open prompt");
  const layout = attentionSurfaceLayout(labels, width, expanded);
  const Icon = levelIcon(model.kind === "waiting" ? "action_needed" : model.kind);
  const controls = <>
    {actions.map((action, index) => <button key={index} type="button" className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} shrink-0`} onClick={() => onAction(action)}>{action.label}</button>)}
    {localActions.map((action, index) => <button key={`local-${index}`} type="button" className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} shrink-0`} onClick={() => { void action.run(); }}>{action.label}</button>)}
    {model.kind === "waiting" && onOpenPrompt && <button type="button" className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} shrink-0`} onClick={onOpenPrompt}>Open prompt</button>}
  </>;
  return (
    <section ref={host} role="region" aria-label={tabAttentionLabel(title, model)} className="relative shrink-0 border-b border-[var(--ct-border)] bg-[var(--ct-surface)] px-3 py-1 text-[12px] text-[var(--ct-muted-fg)]">
      <div className="flex min-w-0 items-center gap-2">
        <Icon size={14} aria-hidden className="shrink-0" />
        <div className="min-w-0 flex-1 truncate text-[var(--ct-fg)]">{model.reason}</div>
        {layout.inlineActions && controls}
        {layout.showChoose && <button type="button" aria-label={`Choose answer for ${model.reason}`} aria-expanded={expanded} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} shrink-0`} onClick={() => setExpandedKey(expanded ? null : key)}>Choose</button>}
        <button type="button" aria-label={`${expanded ? "Hide" : "Show"} attention details for ${title}`} aria-expanded={expanded} aria-controls={`attention-details-${notification?.id ?? title}`} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} shrink-0`} onClick={() => setExpandedKey(expanded ? null : key)}><ChevronDown size={12} aria-hidden /></button>
        <button type="button" aria-label={`Dismiss attention for ${title}`} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} shrink-0`} onClick={onDismiss}><X size={12} aria-hidden /></button>
      </div>
      {layout.showDetails && <div ref={details} id={`attention-details-${notification?.id ?? title}`} className="absolute inset-x-0 top-full z-30 max-h-64 overflow-y-auto border border-[var(--ct-border)] bg-[var(--ct-surface)] p-3 shadow-lg">
        {model.body && <div className="break-words [overflow-wrap:anywhere]"><RichMarkdown text={model.body} /></div>}
        {!layout.inlineActions && <div className="flex flex-wrap gap-1.5">{controls}</div>}
      </div>}
    </section>
  );
}

export function ConnectedTabAttentionBanner({ tab, active, chatCapable }: { tab: Tab; active: boolean; chatCapable: boolean }) {
  const notifications = useMuxSelector((state) => state.notifications);
  const status = useMuxSelector((state) => state.tabStatuses[tab.id]);
  const { markRead, answerNotification } = useMultiplexer();
  const { model, dismiss } = useTabAttention(tab, status, notifications, active, markRead);
  return <TabAttentionBanner title={displayTitleForTab(tab)} model={model} onDismiss={dismiss} onOpenPrompt={chatCapable ? () => emitTabUiMode(tab.id, "chat") : undefined} onAction={(action) => {
    if (action.type === "dismiss") dismiss();
    else if ((action.type === "answer" || action.type === "respond_prompt_option") && model?.notification) answerNotification(model.notification.id, action.label);
    else void runNotificationAction(action);
  }} />;
}
