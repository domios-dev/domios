import type { AgentLevelChoice, AppConfig } from "../../lib/types";
import {
  NOTIFY_DESTINATIONS,
  destinationCellLabel,
  type NotifyDestinationId,
} from "../../lib/notifyDestinations";
import { levelLabel, levelTone } from "../../lib/notifyLevels";
import { Toggle } from "./shared";

export function setLevelDestination(
  current: AgentLevelChoice[],
  level: string,
  destination: NotifyDestinationId,
  routed: boolean,
): AgentLevelChoice[] {
  return current.map((choice) => {
    if (choice.level !== level) return choice;
    const next: AgentLevelChoice = { ...choice };
    next[destination] = routed;
    return next;
  });
}

export function reachesNothing(choice: AgentLevelChoice): boolean {
  return NOTIFY_DESTINATIONS.every((destination) => !choice[destination.id]);
}

/**
 * The levels-by-destinations grid.
 *
 * Rows come from the daemon's own closed level set, so a level added there
 * appears here without anyone editing a list. Columns come from the shared
 * destination set. Every cell is independent, and a level with every column
 * closed is a configuration rather than an error — it simply reaches nothing.
 */
export function NotificationDestinations({
  config,
  onUpdate,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const levels = config.notifications?.agent_levels ?? [];
  if (levels.length === 0) return null;

  return (
    <div className="flex flex-col gap-4" data-testid="settings-notification-destinations">
      {levels.map((choice) => {
        const label = levelLabel(choice.level);
        const tone = levelTone(choice.level);
        return (
          <div
            key={choice.level}
            role="group"
            aria-label={label}
            data-testid="settings-notification-level"
            data-notification-level={choice.level}
            className="flex flex-col gap-2.5"
          >
            <div className="flex items-center gap-2">
              <span
                aria-hidden="true"
                className="inline-flex h-[15px] items-center rounded px-1.5 text-[9px] font-medium tracking-wide uppercase"
                style={{
                  color: tone,
                  background: `color-mix(in srgb, ${tone} 14%, transparent)`,
                }}
              >
                {label}
              </span>
              {reachesNothing(choice) && (
                <span className="text-[10px] text-[var(--ct-muted)]">reaches nothing</span>
              )}
            </div>
            {NOTIFY_DESTINATIONS.map((destination) => (
              <Toggle
                key={destination.id}
                label={destinationCellLabel(label, destination.label)}
                checked={choice[destination.id]}
                onChange={(value) =>
                  onUpdate("notifications", {
                    agent_levels: setLevelDestination(
                      levels,
                      choice.level,
                      destination.id,
                      value,
                    ),
                  })
                }
              />
            ))}
          </div>
        );
      })}
      <p className="text-[10px] text-[var(--ct-muted)] leading-relaxed">
        A message that names no level is kept and rings the bell. It never reaches the system or
        Telegram — there is no fallback level and no way to widen this set from here.
      </p>
    </div>
  );
}
