import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Notification, NotificationAction } from "../../lib/types";
import { InfoIcon, WarningIcon, ErrorIcon } from "../Icons";

const LEVEL_STYLES: Record<Notification["level"], { border: string; icon: string }> = {
  Info: { border: "border-l-[var(--ct-accent)]", icon: "text-[var(--ct-accent)]" },
  Warning: { border: "border-l-[var(--ct-yellow)]", icon: "text-[var(--ct-yellow)]" },
  Error: { border: "border-l-[var(--ct-red)]", icon: "text-[var(--ct-red)]" },
};

const AUTO_DISMISS_MS = 5000;

interface ToastItemProps {
  notification: Notification;
  onDismiss: (id: string) => void;
}

function ToastItem({ notification, onDismiss }: ToastItemProps) {
  const [visible, setVisible] = useState(false);
  const hasActions = notification.actions && notification.actions.length > 0;
  const isSticky = notification.sticky || hasActions;

  useEffect(() => {
    requestAnimationFrame(() => setVisible(true));

    // Auto-dismiss only for non-sticky toasts
    if (!isSticky) {
      const timer = setTimeout(() => {
        setVisible(false);
        setTimeout(() => onDismiss(notification.id), 200);
      }, AUTO_DISMISS_MS);
      return () => clearTimeout(timer);
    }
  }, [notification.id, onDismiss, isSticky]);

  const dismiss = useCallback(() => {
    setVisible(false);
    setTimeout(() => onDismiss(notification.id), 200);
  }, [notification.id, onDismiss]);

  const handleAction = useCallback(async (action: NotificationAction) => {
    switch (action.type) {
      case "invoke":
        if (action.value) {
          try {
            await invoke(action.value, action.args || {});
          } catch (e) {
            console.error("Toast action invoke failed:", e);
          }
        }
        break;
      case "url":
        if (action.value) {
          window.open(action.value, "_blank");
        }
        break;
      case "dismiss":
        break;
    }
    dismiss();
  }, [dismiss]);

  const styles = LEVEL_STYLES[notification.level];

  return (
    <div
      className={`
        border-l-2 ${styles.border} bg-[var(--ct-sidebar-bg)] rounded-r shadow-lg
        px-3 pt-2 pb-1.5 max-w-[360px] min-w-[260px]
        transition-all duration-200 ease-out ct-float
        ${visible ? "opacity-100 translate-x-0" : "opacity-0 translate-x-4"}
      `}
    >
      <div className="flex items-start gap-2">
        <LevelIcon level={notification.level} className={`shrink-0 mt-0.5 ${styles.icon}`} />
        <div className="min-w-0 flex-1">
          <div className="text-xs font-medium text-[var(--ct-fg)] truncate">
            {notification.title}
          </div>
          {notification.body && (
            <div className="text-[11px] text-[var(--ct-muted-fg)] mt-0.5 line-clamp-2">
              {notification.body}
            </div>
          )}

          {/* Action buttons */}
          {hasActions ? (
            <div className="flex items-center gap-2 mt-2">
              {notification.actions!.map((action, i) => (
                <button
                  key={i}
                  onClick={() => handleAction(action)}
                  className={`
                    text-[11px] px-2 py-0.5 rounded transition-colors
                    ${i === 0
                      ? "bg-[var(--ct-accent)] text-white hover:brightness-110"
                      : "text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
                    }
                  `}
                >
                  {action.label}
                </button>
              ))}
            </div>
          ) : (
            /* Click to dismiss for simple toasts */
            <button
              onClick={dismiss}
              className="absolute top-1 right-1.5 text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] text-xs opacity-0 group-hover:opacity-100 transition-opacity"
              aria-label="Dismiss"
            >
              ×
            </button>
          )}
        </div>

        {/* X button for sticky toasts without actions */}
        {isSticky && !hasActions && (
          <button
            onClick={dismiss}
            className="shrink-0 text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] text-sm"
          >
            ×
          </button>
        )}
      </div>
    </div>
  );
}

interface NotificationToastContainerProps {
  toasts: Notification[];
  onDismiss: (id: string) => void;
}

export function NotificationToastContainer({
  toasts,
  onDismiss,
}: NotificationToastContainerProps) {
  if (toasts.length === 0) return null;

  return (
    <div className="fixed top-10 right-3 z-50 flex flex-col gap-2 pointer-events-none">
      {toasts.map((toast) => (
        <div key={toast.id} className="pointer-events-auto">
          <ToastItem notification={toast} onDismiss={onDismiss} />
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
