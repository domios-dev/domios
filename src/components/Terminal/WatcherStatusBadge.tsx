import {
  WATCHER_TONE_STYLES,
  watcherStatusLabel,
  watcherStatusTone,
} from "../../lib/watcherUi";

interface WatcherStatusBadgeProps {
  lastStatus?: string | null;
  className?: string;
}

export function WatcherStatusBadge({ lastStatus, className = "" }: WatcherStatusBadgeProps) {
  const tone = watcherStatusTone(lastStatus);
  const style = WATCHER_TONE_STYLES[tone];
  return (
    <span
      data-testid="watcher-status-badge"
      data-watcher-tone={tone}
      className={`inline-flex items-center rounded-full border px-2 py-0.5 text-[10px] font-mono font-semibold uppercase tracking-wide ${className}`}
      style={{
        color: style.color,
        background: style.background,
        borderColor: style.borderColor,
      }}
    >
      {watcherStatusLabel(lastStatus)}
    </span>
  );
}
