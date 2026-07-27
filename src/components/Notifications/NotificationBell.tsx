import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Bell } from "lucide-react";
import type { Notification } from "../../lib/types";
import { UI_TRAY_BTN } from "../shared/design";
import { NotificationBadge } from "./NotificationBadge";
import { NotificationPanel } from "./NotificationPanel";

interface NotificationBellProps {
  notifications: Notification[];
  onMarkRead: (id: string) => void;
  onMarkAllRead: () => void;
  onDelete: (id: string) => void;
  onNavigate: (paneId: string, workspaceId: string | null) => void;
}

export function NotificationBell({
  notifications,
  onMarkRead,
  onMarkAllRead,
  onDelete,
  onNavigate,
}: NotificationBellProps) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ top: 38, right: 8 });
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const unread = notifications.reduce((count, notification) => count + (notification.read ? 0 : 1), 0);

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
        title="Notifications"
        onClick={toggle}
        className={`${UI_TRAY_BTN} relative ${open ? "text-[var(--ct-accent)]" : ""}`}
      >
        <Bell size={14} />
        <span className="absolute -right-1 -top-0.5">
          <NotificationBadge count={unread} />
        </span>
      </button>
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
            onJumpToPane={(paneId, workspaceId) => {
              onNavigate(paneId, workspaceId);
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
