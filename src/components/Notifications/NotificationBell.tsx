import { useCallback, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Bell } from "lucide-react";
import type { Notification } from "../../lib/types";
import { UI_TRAY_BTN } from "../shared/design";
import { NotificationPanel } from "./NotificationPanel";

interface NotificationBellProps {
  notifications: Notification[];
  onMarkRead: (id: string) => void;
  onMarkAllRead: () => void;
  onDelete: (id: string) => void;
  onClearAll: () => void;
  onNavigate: (tabId: string, workspaceId: string | null) => void;
}

export function NotificationBell({
  notifications,
  onMarkRead,
  onMarkAllRead,
  onDelete,
  onClearAll,
  onNavigate,
}: NotificationBellProps) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ top: 38, right: 8 });
  const [lookedAtId, setLookedAtId] = useState<string | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const unreadStatusId = useId();
  const newestUnread = notifications.reduce<Notification | null>((newest, notification) => {
    if (notification.read) return newest;
    return !newest || notification.timestamp >= newest.timestamp ? notification : newest;
  }, null);
  const newestUnreadId = newestUnread?.id ?? null;
  // The dot means something arrived and you have not looked — never how many.
  const arrived = newestUnreadId !== null && newestUnreadId !== lookedAtId;

  const updatePosition = useCallback(() => {
    const rect = buttonRef.current?.getBoundingClientRect();
    if (!rect) return;
    setPosition({
      top: rect.bottom + 6,
      right: Math.max(8, window.innerWidth - rect.right),
    });
  }, []);

  const toggle = useCallback(() => {
    updatePosition();
    setOpen((value) => !value);
  }, [updatePosition]);

  // Looking is the clear action: an open panel keeps pace with what arrives.
  useEffect(() => {
    if (open) setLookedAtId(newestUnreadId);
  }, [open, newestUnreadId]);

  useEffect(() => {
    if (!open) return;
    const closeOutside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (buttonRef.current?.contains(target) || panelRef.current?.contains(target)) return;
      setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("resize", updatePosition);
    document.addEventListener("pointerdown", closeOutside, true);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("resize", updatePosition);
      document.removeEventListener("pointerdown", closeOutside, true);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open, updatePosition]);

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
      {open && createPortal(
        <div
          ref={panelRef}
          className="fixed z-[70]"
          style={{ top: position.top, right: position.right }}
        >
          <NotificationPanel
            notifications={notifications}
            onMarkRead={onMarkRead}
            onMarkAllRead={onMarkAllRead}
            onDelete={onDelete}
            onClearAll={onClearAll}
            onJumpToTab={(tabId, workspaceId) => {
              onNavigate(tabId, workspaceId);
              setOpen(false);
            }}
            onClose={() => setOpen(false)}
          />
        </div>,
        document.body,
      )}
    </>
  );
}
