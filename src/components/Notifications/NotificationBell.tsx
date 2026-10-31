import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Bell } from "lucide-react";
import type { Notification } from "../../lib/types";
import { UI_TRAY_BTN } from "../shared/design";
import { NotificationPanel } from "./NotificationPanel";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { OPEN_NOTIFICATION_CENTER_EVENT } from "./openNotificationCenter";

interface NotificationBellProps {
  notifications: Notification[];
  loading?: boolean;
  onMarkRead: (id: string) => void;
  onMarkAllRead: () => void;
  onDelete: (id: string) => void;
  onClearAll: () => void;
  onNavigate: (tabId: string, workspaceId: string | null) => void;
  onAnswer?: (id: string, label: string) => void;
}

export function NotificationBell({
  notifications,
  loading = false,
  onMarkRead,
  onMarkAllRead,
  onDelete,
  onClearAll,
  onNavigate,
  onAnswer,
}: NotificationBellProps) {
  const [open, setOpen] = useState(false);
  const [questionId, setQuestionId] = useState<string | undefined>();
  const [lookedAtId, setLookedAtId] = useState<string | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const unreadStatusId = useId();
  const newestUnread = notifications.reduce<Notification | null>((newest, notification) => {
    if (notification.read) return newest;
    return !newest || notification.timestamp >= newest.timestamp ? notification : newest;
  }, null);
  const newestUnreadId = newestUnread?.id ?? null;
  // The dot means something arrived and you have not looked — never how many.
  const arrived = newestUnreadId !== null && newestUnreadId !== lookedAtId;

  const toggle = useCallback(() => setOpen((value) => !value), []);
  const close = useCallback(() => setOpen(false), []);

  useEffect(() => {
    const openPanel = (event: Event) => {
      setQuestionId((event as CustomEvent<{ notificationId?: string }>).detail?.notificationId);
      setOpen(true);
    };
    window.addEventListener(OPEN_NOTIFICATION_CENTER_EVENT, openPanel);
    return () => window.removeEventListener(OPEN_NOTIFICATION_CENTER_EVENT, openPanel);
  }, []);

  // Looking is the clear action: an open panel keeps pace with what arrives.
  useEffect(() => {
    if (open) setLookedAtId(newestUnreadId);
  }, [open, newestUnreadId]);

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        data-testid="notification-bell"
        aria-label="Notifications"
        aria-expanded={open}
        aria-describedby={arrived ? unreadStatusId : undefined}
        title="Notifications"
        onClick={toggle}
        className={`${UI_TRAY_BTN} relative ${open ? "text-[var(--ct-accent)]" : ""}`}
      >
        <Bell
          key={`bell-${newestUnreadId ?? "idle"}`}
          size={14}
          className={arrived ? "ct-notification-bell-ring" : undefined}
        />
        {arrived && (
          <span
            key={`dot-${newestUnreadId}`}
            data-testid="notification-unread-dot"
            aria-hidden="true"
            className="ct-notification-dot-pop absolute right-0 top-0.5 h-1.5 w-1.5 rounded-full bg-[var(--ct-accent)]"
          />
        )}
      </button>
      <span id={unreadStatusId} role="status" className="sr-only">
        {arrived ? "Unread notifications" : ""}
      </span>
      <AnchoredPopover open={open} anchor={buttonRef} placement="bottom-end" gap={6} onClose={close}>
          <NotificationPanel
            questionId={questionId}
            loading={loading}
            notifications={notifications}
            onMarkRead={onMarkRead}
            onMarkAllRead={onMarkAllRead}
            onDelete={onDelete}
            onClearAll={onClearAll}
            onAnswer={onAnswer}
            onJumpToTab={(tabId, workspaceId) => {
              onNavigate(tabId, workspaceId);
              setOpen(false);
            }}
            onClose={close}
          />
      </AnchoredPopover>
    </>
  );
}
