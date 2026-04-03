import { useCallback } from "react";
import type { Notification } from "../../lib/types";
import { CloseXIcon } from "../Icons";
import { timeAgo } from "../../lib/formatters";

const LEVEL_COLORS: Record<Notification["level"], string> = {
  Info: "var(--ct-accent)",
  Warning: "var(--ct-yellow)",
  Error: "var(--ct-red)",
};

interface NotificationPanelProps {
  notifications: Notification[];
  onMarkRead: (id: string) => void;
  onMarkAllRead: () => void;
  onClearAll: () => void;
  onJumpToPane: (paneId: string, workspaceId: string | null) => void;
  onClose: () => void;
}

export function NotificationPanel({
  notifications,
  onMarkRead,
  onMarkAllRead,
  onClearAll,
  onJumpToPane,
  onClose,
}: NotificationPanelProps) {
  const sorted = [...notifications].sort((a, b) => b.timestamp - a.timestamp);

  const handleClick = useCallback(
    (notif: Notification) => {
      if (!notif.read) onMarkRead(notif.id);
      if (notif.pane_id) onJumpToPane(notif.pane_id, notif.workspace_id);
    },
    [onMarkRead, onJumpToPane],
  );

  return (
    <div className="absolute left-[220px] top-0 bottom-0 w-[300px] bg-[var(--ct-sidebar-bg)] border-r border-[var(--ct-border)] z-40 flex flex-col overflow-hidden shadow-xl ct-float">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--ct-border)] shrink-0">
        <span className="text-xs font-semibold uppercase tracking-wider text-[var(--ct-muted-fg)]">
          Notifications
        </span>
        <div className="flex items-center gap-1">
          <button
            onClick={onMarkAllRead}
            className="text-[10px] text-[var(--ct-accent)] hover:text-[var(--ct-accent)] px-1.5 py-0.5 rounded hover:bg-[var(--ct-border)] transition-colors"
            title="Mark all read"
          >
            Read all
          </button>
          <button
            onClick={onClearAll}
            className="text-[10px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] px-1.5 py-0.5 rounded hover:bg-[var(--ct-border)] transition-colors"
            title="Clear all"
          >
            Clear
          </button>
          <button
            onClick={onClose}
            className="flex items-center justify-center w-5 h-5 rounded hover:bg-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors ml-1"
            title="Close panel"
          >
            <CloseXIcon size={8} />
          </button>
        </div>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto">
        {sorted.length === 0 ? (
          <div className="flex items-center justify-center h-24 text-xs text-[var(--ct-muted)]">
            No notifications
          </div>
        ) : (
          sorted.map((notif) => (
            <div
              key={notif.id}
              onClick={() => handleClick(notif)}
              className={`
                flex items-start gap-2 px-3 py-2 cursor-pointer
                border-b border-[var(--ct-border)] hover:bg-[var(--ct-hover)] transition-colors
                ${!notif.read ? "border-l-2 border-l-[var(--ct-accent)]" : "border-l-2 border-l-transparent"}
              `}
            >
              <LevelDot level={notif.level} />
              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-2">
                  <span
                    className={`text-xs truncate ${notif.read ? "text-[var(--ct-muted-fg)]" : "text-[var(--ct-fg)] font-medium"}`}
                  >
                    {notif.title}
                  </span>
                  <span className="text-[10px] text-[var(--ct-muted)] shrink-0">
                    {timeAgo(notif.timestamp)}
                  </span>
                </div>
                {notif.body && (
                  <div className="text-[11px] text-[var(--ct-muted-fg)] mt-0.5 line-clamp-2">
                    {notif.body}
                  </div>
                )}
                {notif.pane_id && (
                  <div className="text-[10px] text-[var(--ct-muted)] mt-0.5">
                    Pane {notif.pane_id.slice(0, 8)}
                  </div>
                )}
              </div>
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
