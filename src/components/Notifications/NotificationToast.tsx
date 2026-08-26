import { useState, useCallback } from "react";
import { createPortal } from "react-dom";
import { RichMarkdown } from "@codeterm/chat-ui";
import { runNotificationAction } from "../../lib/notificationActions";
import type { Notification, NotificationAction } from "../../lib/types";
import { Check, X } from "lucide-react";
import { PluginIcon } from "../Extensions/PluginIcon";
import { notificationLevel, notificationTone } from "../../lib/notifyLevels";
import { levelIcon } from "./levelIcon";

const AUTO_DISMISS_MS = 5000;
const MAX_VISIBLE_TOASTS = 2;

interface ToastItemProps {
  notification: Notification;
  onDismiss: (id: string) => void;
  onMarkRead?: (id: string) => void;
  onNavigate?: (tabId: string, workspaceId: string | null) => void;
  embedded?: boolean;
}

function ToastItem({ notification, onDismiss, onMarkRead, onNavigate, embedded }: ToastItemProps) {
  const [closing, setClosing] = useState(false);
  const [expanded, setExpanded] = useState(false);
  // A choice is answered from the bell and the chat card, which keep it as a
  // log of decisions; a toast that vanishes must not offer one.
  const runnable = notification.actions?.filter((action) => action.type !== "answer") ?? [];
  const hasActions = runnable.length > 0;
  const sticky = notification.sticky || hasActions || expanded;

  const dismiss = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    setClosing(true);
  }, []);

  const handleClick = useCallback(() => {
    if (!notification.read) onMarkRead?.(notification.id);
    setExpanded(true);
  }, [notification.id, notification.read, onMarkRead]);

  const runAction = useCallback(async (action: NotificationAction) => {
    await runNotificationAction(action);
    setClosing(true);
  }, []);

  const handleAnimationEnd = useCallback((event: React.AnimationEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) return;
    if (event.animationName === "ct-toast-life" || event.animationName === "ct-toast-out") {
      onDismiss(notification.id);
    }
  }, [notification.id, onDismiss]);

  const animationClass = closing
    ? "ct-toast-exit"
    : sticky
      ? "ct-toast-enter"
      : "ct-toast-timed";

  const level = notificationLevel(notification);
  const tone = notificationTone(notification);
  const LevelIcon = levelIcon(level);
  const hasCustomIcon = Boolean(notification.icon_svg || notification.icon_hint || notification.icon_color);
  const content = (
    <>
      {hasCustomIcon ? (
        <span className="shrink-0 inline-flex">
          <PluginIcon
            plugin={{
              icon_svg: notification.icon_svg,
              icon_color: notification.icon_color,
              icon_hint: notification.icon_hint,
            }}
            size={16}
          />
        </span>
      ) : (
        <LevelIcon className="shrink-0" style={{ color: tone }} />
      )}
      <div className="min-w-0 flex flex-1 items-baseline gap-1.5">
        <span className="truncate font-medium text-[var(--ct-fg)]">
          {notification.title}
        </span>
        {notification.body && (
          <div className="min-w-0 truncate text-[var(--ct-muted-fg)] [&_p]:m-0 [&_p]:inline">
            <RichMarkdown text={notification.body} />
          </div>
        )}
      </div>
      {hasActions && (
        <div className="flex shrink-0 gap-1">
          {runnable.map((action, i) => (
            <button
              key={i}
              onClick={(e) => { e.stopPropagation(); runAction(action); }}
              className={
                i === 0
                  ? "h-5 rounded border border-[var(--toast-accent)]/35 bg-[var(--toast-accent)]/12 px-1.5 text-[10px] text-[var(--ct-fg)] hover:bg-[var(--toast-accent)]/18"
                  : "h-5 rounded border border-[var(--ct-border)]/70 px-1.5 text-[10px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-bg-hover)] hover:text-[var(--ct-fg)]"
              }
            >
              {action.label}
            </button>
          ))}
        </div>
      )}
      <button
        onClick={dismiss}
        className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-bg-hover)] hover:text-[var(--ct-fg)]"
        aria-label="Dismiss"
      >
        <X size={12} strokeWidth={1.8} />
      </button>
    </>
  );

  if (embedded) {
    return (
      <>
        <div
          onClick={handleClick}
          data-testid="notification-toast"
          data-notification-id={notification.id}
          data-notification-key={notification.key ?? ""}
          data-notification-severity={notification.severity}
          data-notification-level={level ?? ""}
          role="status"
          style={{
            ["--toast-accent" as string]: tone,
            ["--ct-toast-duration" as string]: `${AUTO_DISMISS_MS}ms`,
          }}
          className="ct-toast-inline cursor-pointer"
        >
          <div className={`ct-toast-inline-panel ct-float ${animationClass}`} onAnimationEnd={handleAnimationEnd}>
            <div className="ct-toast-inline-content">
              {content}
            </div>
          </div>
        </div>
        {expanded && createPortal(
          <div
            data-testid="notification-toast-expanded"
            role="dialog"
            aria-label={notification.title}
            className="fixed right-2 top-[calc(2rem+var(--ct-safe-top,0px)+6px)] z-[70] w-[min(420px,calc(100vw-16px))] rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] p-3 shadow-2xl ct-float"
            style={{ ["--toast-accent" as string]: tone }}
            onClick={(event) => event.stopPropagation()}
          >
            <div className="flex items-start gap-2">
              <LevelIcon className="mt-0.5 shrink-0" style={{ color: tone }} />
              <div className="min-w-0 flex-1">
                <div className="font-medium text-[var(--ct-fg)]">{notification.title}</div>
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
                onClick={() => setExpanded(false)}
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

  return (
    <div
      onClick={handleClick}
      data-testid="notification-toast"
      data-notification-id={notification.id}
      data-notification-key={notification.key ?? ""}
      data-notification-severity={notification.severity}
      data-notification-level={level ?? ""}
      role="status"
      style={{
        ["--toast-accent" as string]: tone,
        ["--ct-toast-duration" as string]: `${AUTO_DISMISS_MS}ms`,
      }}
      onAnimationEnd={handleAnimationEnd}
      className={`
        group relative h-7 min-w-0 overflow-hidden
        rounded border border-[var(--ct-border)]/80 bg-[color-mix(in_srgb,var(--ct-surface)_90%,transparent)]
        px-2.5 text-[11px] leading-none shadow-[0_4px_16px_rgba(0,0,0,0.18)]
        ct-float ${hasActions ? "" : "cursor-pointer"}
        ${animationClass}
        max-w-[min(620px,calc(100vw-144px))]
      `}
    >
      <div className={`flex h-full min-w-0 items-center gap-2 ${embedded ? "ct-toast-grow-content" : ""}`}>
        {content}
      </div>
    </div>
  );
}

interface NotificationToastContainerProps {
  toasts: Notification[];
  onDismiss: (id: string) => void;
  onMarkRead?: (id: string) => void;
  onNavigate?: (tabId: string, workspaceId: string | null) => void;
  embedded?: boolean;
}

export function NotificationToastContainer({
  toasts,
  onDismiss,
  onMarkRead,
  onNavigate,
  embedded = false,
}: NotificationToastContainerProps) {
  if (toasts.length === 0) return null;
  const visibleToasts = embedded ? toasts.slice(-1) : toasts.slice(-MAX_VISIBLE_TOASTS);
  const hiddenCount = Math.max(0, toasts.length - visibleToasts.length);

  if (embedded) {
    return (
      <div className="flex h-full min-w-0 max-w-[min(620px,48vw)] items-center gap-1 overflow-hidden pl-1">
        {hiddenCount > 0 && (
          <div className="ct-toast-pending">
            <Check size={10} />
            <span>+{hiddenCount}</span>
          </div>
        )}
        {visibleToasts.map((toast) => (
          <ToastItem
            key={toast.id}
            notification={toast}
            onDismiss={onDismiss}
            onMarkRead={onMarkRead}
            onNavigate={onNavigate}
            embedded
          />
        ))}
      </div>
    );
  }

  return (
    <div className="fixed left-1/2 top-2 z-40 flex w-[min(620px,calc(100vw-160px))] -translate-x-1/2 flex-col items-center gap-1 pointer-events-none">
      {hiddenCount > 0 && (
        <div className="pointer-events-none flex h-5 items-center gap-1 rounded border border-[var(--ct-border)]/70 bg-[var(--ct-surface)]/80 px-2 text-[10px] text-[var(--ct-muted-fg)] ct-float">
          <Check size={11} />
          <span>+{hiddenCount}</span>
        </div>
      )}
      {visibleToasts.map((toast) => (
        <div key={toast.id} className="pointer-events-auto">
          <ToastItem notification={toast} onDismiss={onDismiss} onMarkRead={onMarkRead} onNavigate={onNavigate} />
        </div>
      ))}
    </div>
  );
}
