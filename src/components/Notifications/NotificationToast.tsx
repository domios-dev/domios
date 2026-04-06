import { useEffect, useState, useCallback } from "react";
import type { Notification } from "../../lib/types";
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
  onNavigate?: (paneId: string) => void;
}

function ToastItem({ notification, onDismiss, onNavigate }: ToastItemProps) {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    requestAnimationFrame(() => setVisible(true));

    const timer = setTimeout(() => {
      setVisible(false);
      setTimeout(() => onDismiss(notification.id), 200);
    }, AUTO_DISMISS_MS);
    return () => clearTimeout(timer);
  }, [notification.id, onDismiss]);

  const dismiss = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    setVisible(false);
    setTimeout(() => onDismiss(notification.id), 200);
  }, [notification.id, onDismiss]);

  const handleClick = useCallback(() => {
    if (notification.pane_id && onNavigate) {
      onNavigate(notification.pane_id);
    }
    setVisible(false);
    setTimeout(() => onDismiss(notification.id), 200);
  }, [notification.id, notification.pane_id, onDismiss, onNavigate]);

  const styles = LEVEL_STYLES[notification.level];

  return (
    <div
      onClick={handleClick}
      className={`
        border-l-2 ${styles.border} bg-[var(--ct-sidebar-bg)] rounded-r shadow-lg
        px-3 pt-2 pb-1.5 max-w-[360px] min-w-[260px]
        transition-all duration-200 ease-out ct-float cursor-pointer
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
