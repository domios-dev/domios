import { useCallback } from "react";
import type { Notification } from "../../lib/types";
import { CloseXIcon } from "../Icons";
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
  const headerBtnClass = `${UI_BUTTON.ghost} ${UI_BUTTON.compact} text-[var(--ct-accent)]`;
  const headerActionClass = `${UI_BUTTON.ghost} ${UI_BUTTON.compact}`;
  const rowClass = `flex items-start gap-2 px-3 py-2 cursor-pointer border-b border-[var(--ct-border)] hover:bg-[var(--ct-hover)] transition-colors`;

  return (
    <div className="absolute left-[220px] top-0 bottom-0 w-[300px] bg-[var(--ct-sidebar-bg)] border-r border-[var(--ct-border)] z-40 flex flex-col overflow-hidden shadow-xl ct-float">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--ct-border)] shrink-0">
        <span className="text-sm font-medium text-[var(--ct-muted-fg)]">
          Notifications
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
            onClick={onClearAll}
            className={headerActionClass}
            title="Clear all"
          >
            Clear
          </button>
          <button
            onClick={onClose}
            className="ct-btn ct-btn-ghost w-7 h-7"
            title="Close panel"
          >
            <CloseXIcon size={8} />
          </button>
        </div>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto">
        {sorted.length === 0 ? (
          <div className="flex items-center justify-center h-24">
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
                  <span className="text-sm text-[var(--ct-muted)] shrink-0">
                    {timeAgo(notif.timestamp)}
                  </span>
                </div>
                {notif.body && (
                  <div className={`${UI_TEXT.meta} mt-0.5 line-clamp-2`}>
                    {notif.body}
                  </div>
                )}
                {notif.pane_id && (
                  <div className="text-sm text-[var(--ct-muted)] mt-0.5">
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
