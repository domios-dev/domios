import { useState, useCallback, useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { RichMarkdown } from "@codeterm/chat-ui";
import { runNotificationAction } from "../../lib/notificationActions";
import type { Notification, NotificationAction } from "../../lib/types";
import { X } from "lucide-react";
import { PluginIcon } from "../Extensions/PluginIcon";
import { notificationLevel, notificationTone } from "../../lib/notifyLevels";
import { RunPath } from "../Automation/RunPath";
import { requestOpenJob } from "../Jobs/openJobEvent";
import { levelIcon } from "./levelIcon";
import { useReducedMotion } from "../Canvas/missionControl/useReducedMotion";
import { countdownAnimation } from "./toastLifetime";
import { useToastHostWidth } from "./useToastHostWidth";
import { useToastLifetime } from "./useToastLifetime";

const MAX_VISIBLE_TOASTS = 3;

interface ToastItemProps {
  notification: Notification;
  onDismiss: (id: string) => void;
  onMarkRead?: (id: string) => void;
  onNavigate?: (tabId: string, workspaceId: string | null) => void;
}

function ToastItem({ notification, onDismiss, onMarkRead, onNavigate }: ToastItemProps) {
  const reducedMotion = useReducedMotion();
  const lifetime = useToastLifetime(notification.id, notification.lifetime_ms, onDismiss);
  const [expanded, setExpanded] = useState(false);
  const expandedPanel = useRef<HTMLDivElement>(null);
  const runOpener = useRef<HTMLButtonElement>(null);
  const detailsOpener = useRef<HTMLButtonElement>(null);
  const isRun = !!notification.blueprint_run;
  useLayoutEffect(() => {
    if (expanded) expandedPanel.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [expanded]);
  const closeExpanded = () => { setExpanded(false); (isRun ? runOpener : detailsOpener).current?.focus(); };
  // A choice is answered from the bell and the chat card, which keep it as a
  // log of decisions; a toast that vanishes must not offer one.
  const runnable = notification.actions?.filter((action) =>
    action.type !== "answer" && action.type !== "respond_prompt_option",
  ) ?? [];
  const hasActions = runnable.length > 0;
  const dismiss = (event: React.MouseEvent) => {
    event.stopPropagation();
    lifetime.dismiss();
  };

  const handleClick = useCallback(() => {
    if (!notification.read && !notification.blueprint_run) onMarkRead?.(notification.id);
    setExpanded(true);
  }, [notification.id, notification.read, notification.blueprint_run, onMarkRead]);

  const runAction = async (action: NotificationAction) => {
    await runNotificationAction(action);
    lifetime.dismiss();
  };

  const level = notificationLevel(notification);
  const tone = notificationTone(notification);
  const LevelIcon = levelIcon(level);
  const hasCustomIcon = Boolean(notification.icon_svg);
  const content = (
    <>
      {hasCustomIcon ? (
        <span className="shrink-0 inline-flex">
          <PluginIcon
            plugin={{
              icon_svg: notification.icon_svg,
            }}
            size={16}
          />
        </span>
      ) : (
        <LevelIcon className="shrink-0" style={{ color: tone }} />
      )}
      <div className="min-w-0 flex flex-1 items-center gap-1.5 overflow-hidden whitespace-nowrap">
        <button ref={detailsOpener} type="button" aria-label={`Show notification: ${notification.title}`} className="block min-w-0 max-w-full truncate text-left font-medium text-[var(--ct-fg)]" onClick={(event) => { event.stopPropagation(); handleClick(); }}>
          {notification.title}
        </button>
        {notification.body && (
          <div className="min-w-0 flex-1 truncate text-[var(--ct-muted-fg)] [&_*]:whitespace-nowrap [&_p]:m-0 [&_p]:inline">
            <RichMarkdown text={notification.body} />
          </div>
        )}
      </div>
      {notification.blueprint_run && <button ref={runOpener} type="button" className="shrink-0 text-[10px] text-[var(--ct-accent)]" onClick={(event) => { event.stopPropagation(); setExpanded(true); }}>View run</button>}
      {notification.local_actions?.map((action) => <button key={action.label} type="button" className="shrink-0 text-[10px] text-[var(--ct-accent)]" onClick={(event) => { event.stopPropagation(); void action.run(); lifetime.dismiss(); }}>{action.label}</button>)}
      {hasActions && (
        <div className="flex min-w-0 max-w-[60%] gap-1 overflow-x-auto">
          {runnable.map((action, i) => (
            <button
              key={i}
              onClick={(e) => { e.stopPropagation(); runAction(action); }}
              className={
                i === 0
                  ? "h-5 max-w-[132px] truncate rounded border border-[var(--toast-accent)]/35 bg-[var(--toast-accent)]/12 px-1.5 text-[10px] text-[var(--ct-fg)] hover:bg-[var(--toast-accent)]/18"
                  : "h-5 max-w-[132px] truncate rounded border border-[var(--ct-border)]/70 px-1.5 text-[10px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-bg-hover)] hover:text-[var(--ct-fg)]"
              }
            >
              {action.label}
            </button>
          ))}
        </div>
      )}
      <button
        onClick={dismiss}
        className="relative inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-bg-hover)] hover:text-[var(--ct-fg)]"
        aria-label="Dismiss"
      >
        <svg aria-hidden="true" viewBox="0 0 24 24" className="pointer-events-none absolute inset-0 h-6 w-6 -rotate-90">
          <circle cx="12" cy="12" r="10" pathLength="1" fill="none" stroke="currentColor" strokeWidth="1" strokeDasharray="1" className="ct-toast-countdown" style={{ animationName: countdownAnimation(reducedMotion), animationDuration: `${lifetime.duration}ms`, animationPlayState: lifetime.paused ? "paused" : "running" }} />
        </svg>
        <X size={12} strokeWidth={1.8} />
      </button>
    </>
  );

  return (
    <>
      <div
        onClick={handleClick}
        data-testid="notification-toast"
        data-notification-id={notification.id}
        data-notification-key={notification.key ?? ""}
        data-notification-severity={notification.severity}
        data-notification-level={level ?? ""}
        onMouseEnter={lifetime.pause}
        onMouseLeave={lifetime.resume}
        role="status"
        style={{
          ["--toast-accent" as string]: tone,
          ["--ct-toast-duration" as string]: `${lifetime.duration}ms`,
        }}
        className="ct-toast-inline cursor-pointer"
      >
        <div className="ct-toast-inline-panel ct-float">
          <div className="ct-toast-inline-content">
            {content}
          </div>
        </div>
      </div>
      {expanded && createPortal(
        <div
          ref={expandedPanel}
          data-testid="notification-toast-expanded"
          role="dialog"
          aria-label={notification.title}
          className="fixed right-2 top-[calc(2rem+var(--ct-safe-top,0px)+6px)] z-[70] w-[min(420px,calc(100vw-16px))] rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] p-3 shadow-2xl ct-float"
          style={{ ["--toast-accent" as string]: tone }}
          onKeyDown={(event) => { if (event.code === "Escape") { event.stopPropagation(); closeExpanded(); } }}
          onClick={(event) => event.stopPropagation()}
        >
          <div className="flex items-start gap-2">
            <LevelIcon className="mt-0.5 shrink-0" style={{ color: tone }} />
            <div className="min-w-0 flex-1">
              <div className="font-medium text-[var(--ct-fg)]">{notification.title}</div>
              {notification.blueprint_run && <>
                <RunPath run={notification.blueprint_run.run} />
                <button type="button" className="mt-2 rounded border border-[var(--ct-border)] px-2 py-1 text-xs text-[var(--ct-fg)]" onClick={() => {
                  const data = notification.blueprint_run!;
                  requestOpenJob({ workspaceId: data.workspaceId, target: { teamId: data.teamId, scenarioId: data.scenarioId, runId: data.run.key } });
                  setExpanded(false);
                }}>Open full graph</button>
              </>}
              {notification.body && (
                <div className="mt-2 max-h-[min(420px,60vh)] overflow-y-auto break-words text-[12px] leading-5 text-[var(--ct-muted-fg)]">
                  <RichMarkdown text={notification.body} />
                </div>
              )}
              <div className="mt-3 flex flex-wrap gap-1.5">
                {notification.tab_id && onNavigate && (
                  <button
                    type="button"
                    className="rounded border border-[var(--ct-border)] px-2 py-1 text-[10px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                    onClick={() => onNavigate(notification.tab_id!, notification.workspace_id)}
                  >
                    Show Tab
                  </button>
                )}
                {runnable.map((action, index) => (
                  <button
                    type="button"
                    key={`${action.label}-${index}`}
                    className="rounded border border-[var(--ct-border)] px-2 py-1 text-[10px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                    onClick={() => runAction(action)}
                  >
                    {action.label}
                  </button>
                ))}
              </div>
            </div>
            <button
              type="button"
              aria-label="Close expanded notification"
              className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
              onClick={closeExpanded}
            >
              <X size={13} />
            </button>
          </div>
        </div>,
        document.body,
      )}
    </>
  );
}

interface NotificationToastContainerProps {
  toasts: Notification[];
  onDismiss: (id: string) => void;
  onMarkRead?: (id: string) => void;
  onNavigate?: (tabId: string, workspaceId: string | null) => void;
}

export function NotificationToastContainer({
  toasts,
  onDismiss,
  onMarkRead,
  onNavigate,
}: NotificationToastContainerProps) {
  const hostRef = useToastHostWidth(toasts.length);
  if (toasts.length === 0) return null;
  const visibleToasts = toasts.slice(-MAX_VISIBLE_TOASTS).reverse();
  return (
    <div ref={hostRef} data-testid="toast-host" aria-label="Notifications" className="absolute right-0 top-full z-[65] mt-1.5 flex w-[min(420px,calc(100vw-16px))] flex-col gap-1.5 pointer-events-none">
      {visibleToasts.map((toast) => <div key={toast.id} className="pointer-events-auto">
        <ToastItem notification={toast} onDismiss={onDismiss} onMarkRead={onMarkRead} onNavigate={onNavigate} />
      </div>)}
    </div>
  );
}
