import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Notification, NotificationAction } from "../../lib/types";

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
    case "Info":
      return (
        <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" className={className}>
          <path d="M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z" />
        </svg>
      );
    case "Warning":
      return (
        <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" className={className}>
          <path d="M6.457 1.047c.659-1.234 2.427-1.234 3.086 0l6.082 11.378A1.75 1.75 0 0 1 14.082 15H1.918a1.75 1.75 0 0 1-1.543-2.575ZM8 5a.75.75 0 0 0-.75.75v2.5a.75.75 0 0 0 1.5 0v-2.5A.75.75 0 0 0 8 5Zm0 9a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z" />
        </svg>
      );
    case "Error":
      return (
        <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" className={className}>
          <path d="M2.343 13.657A8 8 0 1 1 13.657 2.343 8 8 0 0 1 2.343 13.657ZM6.03 4.97a.751.751 0 0 0-1.042.018.751.751 0 0 0-.018 1.042L6.94 8 4.97 9.97a.749.749 0 0 0 .326 1.275.749.749 0 0 0 .734-.217L8 9.06l1.97 1.97a.749.749 0 0 0 1.275-.326.749.749 0 0 0-.215-.734L9.06 8l1.97-1.97a.749.749 0 0 0-.326-1.275.749.749 0 0 0-.734.217L8 6.94Z" />
        </svg>
      );
  }
}
