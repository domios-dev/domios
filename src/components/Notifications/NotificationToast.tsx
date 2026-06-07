import { useEffect, useState, useCallback } from "react";
import { invoke } from "../../lib/bridge";
import type { Notification, NotificationAction } from "../../lib/types";
import { InfoIcon, WarningIcon, ErrorIcon } from "../Icons";
import { UI_BUTTON } from "../shared/design";

const LEVEL_STYLES: Record<Notification["level"], { border: string; icon: string }> = {
  Info: { border: "border-l-[var(--ct-accent)]", icon: "text-[var(--ct-accent)]" },
  Warning: { border: "border-l-[var(--ct-yellow)]", icon: "text-[var(--ct-yellow)]" },
  Error: { border: "border-l-[var(--ct-red)]", icon: "text-[var(--ct-red)]" },
};

const AUTO_DISMISS_MS = 5000;

interface ToastItemProps {
  notification: Notification;
  onDismiss: (id: string) => void;
  onNavigate?: (paneId: string) => void;
}

function ToastItem({ notification, onDismiss, onNavigate }: ToastItemProps) {
  const [visible, setVisible] = useState(false);
  const hasActions = (notification.actions?.length ?? 0) > 0;
  const sticky = notification.sticky || hasActions;

  useEffect(() => {
    requestAnimationFrame(() => setVisible(true));
    if (sticky) return;
    const timer = setTimeout(() => {
      setVisible(false);
      setTimeout(() => onDismiss(notification.id), 200);
    }, AUTO_DISMISS_MS);
    return () => clearTimeout(timer);
  }, [notification.id, onDismiss, sticky]);

  const dismiss = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    setVisible(false);
    setTimeout(() => onDismiss(notification.id), 200);
  }, [notification.id, onDismiss]);

  const handleClick = useCallback(() => {
    if (hasActions) return;
    if (notification.pane_id && onNavigate) {
      onNavigate(notification.pane_id);
    }
    setVisible(false);
    setTimeout(() => onDismiss(notification.id), 200);
  }, [notification.id, notification.pane_id, onDismiss, onNavigate, hasActions]);

  const runAction = useCallback(async (action: NotificationAction) => {
    if (action.type === "dismiss") {
      // just close
    } else if (action.type === "url" && action.value) {
      try { window.open(action.value, "_blank"); } catch {}
    } else if (action.type === "invoke" && action.value) {
      try {
        await invoke(action.value, action.args ?? {});
      } catch (e) {
        console.warn(`Notification action '${action.value}' failed:`, e);
      }
    }
    setVisible(false);
    setTimeout(() => onDismiss(notification.id), 200);
  }, [notification.id, onDismiss]);

  const styles = LEVEL_STYLES[notification.level];

  return (
    <div
      onClick={handleClick}
      className={`
        border-l-2 ${styles.border} bg-[var(--ct-sidebar-bg)] rounded-r shadow-lg
        px-3 pt-2 pb-1.5 max-w-[360px] min-w-[260px]
        transition-all duration-200 ease-out ct-float ${hasActions ? "" : "cursor-pointer"}
        ${visible ? "opacity-100 translate-x-0" : "opacity-0 translate-x-4"}
      `}
    >
          <div className="flex items-start gap-2">
            <LevelIcon level={notification.level} className={`shrink-0 mt-0.5 ${styles.icon}`} />
            <div className="min-w-0 flex-1">
              <div className="text-sm font-medium text-[var(--ct-fg)] truncate">
                {notification.title}
              </div>
              {notification.body && (
                <div className="text-sm text-[var(--ct-muted-fg)] mt-0.5 line-clamp-2 leading-relaxed">
                  {notification.body}
                </div>
              )}
              {hasActions && (
                <div className="flex gap-1.5 mt-2">
                  {notification.actions!.map((action, i) => (
                    <button
                      key={i}
                      onClick={(e) => { e.stopPropagation(); runAction(action); }}
                      className={`${i === 0 ? UI_BUTTON.primary : UI_BUTTON.ghost} px-2 py-1`}
                    >
                      {action.label}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <button
              onClick={dismiss}
              className="shrink-0 text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] text-sm leading-none mt-0.5"
              aria-label="Dismiss"
            >
          ×
        </button>
      </div>
    </div>
  );
}

interface NotificationToastContainerProps {
  toasts: Notification[];
  onDismiss: (id: string) => void;
  onNavigate?: (paneId: string) => void;
}

export function NotificationToastContainer({
  toasts,
  onDismiss,
  onNavigate,
}: NotificationToastContainerProps) {
  if (toasts.length === 0) return null;

  return (
    <div className="fixed top-10 right-3 z-50 flex flex-col gap-2 pointer-events-none">
      {toasts.map((toast) => (
        <div key={toast.id} className="pointer-events-auto">
          <ToastItem notification={toast} onDismiss={onDismiss} onNavigate={onNavigate} />
        </div>
      ))}
    </div>
  );
}

function LevelIcon({ level, className }: { level: Notification["level"]; className?: string }) {
  switch (level) {
    case "Info": return <InfoIcon className={className} />;
    case "Warning": return <WarningIcon className={className} />;
    case "Error": return <ErrorIcon className={className} />;
  }
}
