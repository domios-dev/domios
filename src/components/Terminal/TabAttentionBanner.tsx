import { RichMarkdown } from "@codeterm/chat-ui";
import { X } from "lucide-react";
import type { NotificationAction, Tab } from "../../lib/types";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import { emitTabUiMode } from "../../lib/chatUiMode";
import { runNotificationAction } from "../../lib/notificationActions";
import { UI_BUTTON } from "../shared/design";
import { attentionActions, tabAttentionLabel, type TabAttention } from "../Sidebar/tabAttention";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { useTabAttention } from "./useTabAttention";

export function TabAttentionBanner({ title, model, onDismiss, onAction, onOpenPrompt }: {
  title: string;
  model: TabAttention | null;
  onDismiss: () => void;
  onAction: (action: NotificationAction) => void;
  onOpenPrompt?: () => void;
}) {
  if (!model) return null;
  const notification = model.notification;
  return (
    <section role="region" aria-label={tabAttentionLabel(title, model)} className="shrink-0 border-b border-[var(--ct-border)] bg-[var(--ct-surface)] px-3 py-2 text-[12px] text-[var(--ct-muted-fg)]">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="text-[var(--ct-fg)]">{model.reason}</div>
          {model.body && <div className="max-h-32 overflow-y-auto break-words"><RichMarkdown text={model.body} /></div>}
          <div className="flex flex-wrap gap-1.5">
            {attentionActions(notification).map((action, index) => <button key={index} type="button" className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`} onClick={() => onAction(action)}>{action.label}</button>)}
            {!notification?.resolved && notification?.local_actions?.map((action, index) => <button key={`local-${index}`} type="button" className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`} onClick={() => { void action.run(); }}>{action.label}</button>)}
            {model.kind === "waiting" && onOpenPrompt && <button type="button" className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`} onClick={onOpenPrompt}>Open prompt</button>}
          </div>
        </div>
        <button type="button" aria-label={`Dismiss attention for ${title}`} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`} onClick={onDismiss}><X size={12} aria-hidden /></button>
      </div>
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
