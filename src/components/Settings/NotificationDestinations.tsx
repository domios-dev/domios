import type { AgentLevelChoice, AppConfig } from "../../lib/types";
import {
  destinationCellLabel,
  destinationColumns,
  isRouted,
  type DestinationColumn,
  type PushChannelDescriptor,
} from "../../lib/notifyDestinations";
import { levelLabel, levelTone } from "../../lib/notifyLevels";
import {
  channelPatch,
  channelRecipients,
  configuredUsers,
  preselectedAudience,
} from "../../lib/pushChannels";
import { Toggle } from "./shared";

export function setLevelDestination(
  current: AgentLevelChoice[],
  level: string,
  destination: string,
  routed: boolean,
): AgentLevelChoice[] {
  return current.map((choice) => {
    if (choice.level !== level) return choice;
    return { ...choice, [destination]: routed };
  });
}

/**
 * One cell, flipped. Opening a channel's column is the whole of whether that
 * level reaches the channel; it also answers who when the answer is
 * unambiguous, so a single configured id needs nothing typed. With several the
 * audience stays untouched — a preselected everyone would make one flip a
 * broadcast — and an audience already chosen is never overwritten.
 */
export function destinationCellUpdate(
  config: AppConfig,
  levels: AgentLevelChoice[],
  level: string,
  column: DestinationColumn,
  routed: boolean,
): Partial<AppConfig["notifications"]> {
  const agent_levels = setLevelDestination(levels, level, column.id, routed);
  if (column.kind !== "channel" || !routed) return { agent_levels };
  if (channelRecipients(config, column.id).length > 0) return { agent_levels };
  const preselected = preselectedAudience(configuredUsers(config, column.id));
  if (preselected.length === 0) return { agent_levels };
  return {
    agent_levels,
    ...channelPatch(config.notifications?.channels, column.id, {
      recipients: preselected,
    }),
  };
}

export function reachesNothing(
  choice: AgentLevelChoice,
  columns: readonly DestinationColumn[],
): boolean {
  return columns.every((column) => !isRouted(choice, column.id));
}

/**
 * The levels-by-destinations grid.
 *
 * Rows come from the daemon's own closed level set, so a level added there
 * appears here without anyone editing a list. Columns come from the daemon's
 * channel registry the same way: this component never names a channel, so a
 * channel the registry gains becomes a column with nothing here to change.
 * Every cell is independent, and a level with every column closed is a
 * configuration rather than an error — it simply reaches nothing. A channel's
 * column is the whole of whether that level reaches it; who receives it is that
 * channel's own audience surface, never a second switch agreeing with this one.
 */
export function NotificationDestinations({
  config,
  channels,
  onUpdate,
}: {
  config: AppConfig;
  channels: readonly PushChannelDescriptor[];
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const levels = config.notifications?.agent_levels ?? [];
  if (levels.length === 0) return null;
  const columns = destinationColumns(channels);

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
              {reachesNothing(choice, columns) && (
                <span className="text-[10px] text-[var(--ct-muted)]">reaches nothing</span>
              )}
            </div>
            {columns.map((destination) => (
              <Toggle
                key={destination.id}
                label={destinationCellLabel(label, destination.label)}
                checked={isRouted(choice, destination.id)}
                onChange={(value) =>
                  onUpdate(
                    "notifications",
                    destinationCellUpdate(config, levels, choice.level, destination, value),
                  )
                }
              />
            ))}
          </div>
        );
      })}
      <p className="text-[10px] text-[var(--ct-muted)] leading-relaxed">
        A message that names no level is kept and rings the bell. It never reaches the system or
        any push channel — there is no fallback level and no way to widen this set from here.
      </p>
    </div>
  );
}
