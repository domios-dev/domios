import { useCallback } from "react";
import type { Notification } from "../../lib/types";
import { Bell, Trash2, X } from "lucide-react";
import { timeAgo } from "../../lib/formatters";
import { UI_BUTTON, UI_TEXT } from "../shared/design";

const LEVEL_COLORS: Record<Notification["level"], string> = {
  Info: "var(--ct-accent)",
  Warning: "var(--ct-yellow)",
  Error: "var(--ct-red)",
};

interface NotificationPanelProps {
  notifications: Notification[];
  onMarkRead: (id: string) => void;
  onMarkAllRead: () => void;
  onDelete: (id: string) => void;
  onJumpToPane: (paneId: string, workspaceId: string | null) => void;
  onClose: () => void;
}

export function NotificationPanel({
  notifications,
  onMarkRead,
  onMarkAllRead,
  onDelete,
  onJumpToPane,
  onClose,
}: NotificationPanelProps) {
  const sorted = [...notifications].sort((a, b) => b.timestamp - a.timestamp);

  const handleClick = useCallback(
    (notif: Notification) => {
      if (!notif.read) onMarkRead(notif.id);
    },
    [onMarkRead],
  );
  const headerBtnClass = `${UI_BUTTON.ghost} ${UI_BUTTON.compact} text-[var(--ct-accent)]`;
  const rowClass = `group flex items-start gap-2 border-b border-[var(--ct-border)] px-3 py-2.5 transition-colors hover:bg-[var(--ct-hover)]`;

  return (
    <div data-testid="notification-panel" className="flex max-h-[min(560px,calc(100vh-48px))] w-[min(380px,calc(100vw-16px))] flex-col overflow-hidden rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] shadow-2xl ct-float">
      <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--ct-border)] shrink-0">
        <span className="flex items-center gap-2 text-sm font-medium text-[var(--ct-fg)]">
          <Bell size={14} /> Notifications
        </span>
        <div className="flex items-center gap-1">
          <button
            onClick={onMarkAllRead}
            className={`${headerBtnClass} border-[color-mix(in srgb, var(--ct-accent) 35%, var(--ct-border) 65%)]`}
            title="Mark all read"
          >
            Read all
          </button>
          <button
            onClick={onClose}
            className="ct-btn ct-btn-ghost w-7 h-7"
            aria-label="Close notifications"
          >
            <X size={13} />
          </button>
        </div>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto">
        {sorted.length === 0 ? (
          <div className="flex h-28 flex-col items-center justify-center gap-2">
            <Bell size={18} className="text-[var(--ct-muted)]" />
            <span className={UI_TEXT.metaMuted}>No notifications</span>
          </div>
        ) : (
          sorted.map((notif) => (
            <div
              key={notif.id}
              onClick={() => handleClick(notif)}
              className={`${rowClass} ${!notif.read ? "border-l-2 border-l-[var(--ct-accent)]" : "border-l-2 border-l-transparent"}`}
            >
              <LevelDot level={notif.level} />
              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-2">
                  <span
                    className={`${notif.read ? UI_TEXT.meta : "font-medium text-[var(--ct-fg)]"} truncate`}
                  >
                    {notif.title}
                  </span>
                  <span className="shrink-0 text-[10px] text-[var(--ct-muted)]">
                    {timeAgo(notif.timestamp)}
                  </span>
                </div>
                {notif.body && (
                  <div className="mt-1 whitespace-pre-wrap break-words text-[12px] leading-5 text-[var(--ct-muted-fg)]">
                    {notif.body}
                  </div>
                )}
                {notif.pane_id && (
                  <button
                    type="button"
                    className="mt-1.5 rounded border border-[var(--ct-border)] px-2 py-1 text-[10px] text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                    onClick={(event) => {
                      event.stopPropagation();
                      if (!notif.read) onMarkRead(notif.id);
                      onJumpToPane(notif.pane_id!, notif.workspace_id);
                    }}
                  >
                    Show Tab
                  </button>
                )}
              </div>
              <button
                type="button"
                aria-label={`Delete ${notif.title}`}
                title="Delete"
                className="mt-0.5 inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--ct-muted)] opacity-0 hover:bg-[var(--ct-hover)] hover:text-[var(--ct-red)] group-hover:opacity-100 focus:opacity-100"
                onClick={(event) => {
                  event.stopPropagation();
                  onDelete(notif.id);
                }}
              >
                <Trash2 size={12} />
              </button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function LevelDot({ level }: { level: Notification["level"] }) {
  return (
    <span
      className="inline-block w-2 h-2 rounded-full shrink-0 mt-1"
      style={{ backgroundColor: LEVEL_COLORS[level] }}
    />
  );
}
