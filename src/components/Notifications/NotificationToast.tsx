import { useEffect, useState } from "react";
import type { Notification } from "../../lib/types";

const LEVEL_STYLES: Record<Notification["level"], { border: string; icon: string }> = {
  Info: { border: "border-l-[#58a6ff]", icon: "text-[#58a6ff]" },
  Warning: { border: "border-l-[#d29922]", icon: "text-[#d29922]" },
  Error: { border: "border-l-[#f85149]", icon: "text-[#f85149]" },
};

interface ToastItemProps {
  notification: Notification;
  onDismiss: (id: string) => void;
}

function ToastItem({ notification, onDismiss }: ToastItemProps) {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    // Trigger enter animation
    requestAnimationFrame(() => setVisible(true));

    const timer = setTimeout(() => {
      setVisible(false);
      setTimeout(() => onDismiss(notification.id), 200);
    }, 5000);

    return () => clearTimeout(timer);
  }, [notification.id, onDismiss]);

  const styles = LEVEL_STYLES[notification.level];

  return (
    <div
      onClick={() => {
        setVisible(false);
        setTimeout(() => onDismiss(notification.id), 200);
      }}
      className={`
        border-l-2 ${styles.border} bg-[#161b22] rounded-r shadow-lg
        px-3 py-2 cursor-pointer max-w-[320px] min-w-[240px]
        transition-all duration-200 ease-out
        ${visible ? "opacity-100 translate-x-0" : "opacity-0 translate-x-4"}
      `}
    >
      <div className="flex items-start gap-2">
        <LevelIcon level={notification.level} className={`shrink-0 mt-0.5 ${styles.icon}`} />
        <div className="min-w-0">
          <div className="text-xs font-medium text-[#c9d1d9] truncate">
            {notification.title}
          </div>
          {notification.body && (
            <div className="text-[11px] text-[#8b949e] mt-0.5 line-clamp-2">
              {notification.body}
            </div>
          )}
        </div>
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
