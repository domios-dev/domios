import type { AgentLevelChoice, AppConfig } from "../../lib/types";
import {
  PUSH_DESTINATION_LABEL,
  destinationCellLabel,
  destinationColumns,
  destinationSummary,
  isRouted,
  localDestinations,
  pushDestinations,
  pushPickerLabel,
  setLevelDestination,
  type DestinationColumn,
  type PushChannelDescriptor,
} from "../../lib/notifyDestinations";
import { levelLabel, levelMeaning, levelTone } from "../../lib/notifyLevels";
import {
  channelPatch,
  channelRecipients,
  configuredUsers,
  preselectedAudience,
} from "../../lib/pushChannels";
import { DestinationPicker, type DestinationOption } from "./DestinationPicker";
import { Toggle } from "./shared";

/**
 * One destination, moved. Opening a channel is the whole of whether that level
 * reaches the channel; it also answers who when the answer is unambiguous, so a
 * single admitted id needs nothing confirmed. With several the audience stays
 * untouched — a preselected everyone would make one pick a broadcast — and an
 * audience already chosen is never overwritten.
 */
export function destinationCellUpdate(
  config: AppConfig,
  levels: AgentLevelChoice[],
  level: string,
  column: DestinationColumn,
  routed: boolean,
): Partial<AppConfig["notifications"]> {
  const agent_levels = setLevelDestination(levels, level, column, routed);
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

/** Every column closed. The record is still kept and still reaches the panel —
 *  these columns are the destinations that INTERRUPT, and naming none of them is
 *  a configuration rather than an error. */
export function reachesNothing(
  choice: AgentLevelChoice,
  columns: readonly DestinationColumn[],
): boolean {
  return columns.every((column) => !isRouted(choice, column.id));
}

/**
 * Where each level may go.
 *
 * Rows come from the daemon's own closed level set, so a level added there
 * appears here without anyone editing a list. The system notification is a fixed
 * surface and keeps its toggle. The push side is not a fixed set — it is a
 * registry — so it is ONE control per level that names the channels that level
 * reaches, and a channel the registry gains widens that control instead of
 * adding a row to every level. Only a channel that can carry a message is
 * offered. A level that picks nothing is a configuration rather than an error:
 * it is kept in the panel and interrupts nowhere.
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
  const local = localDestinations(columns);
  const push = pushDestinations(columns);

  return (
    <div className="flex flex-col gap-4" data-testid="settings-notification-destinations">
      {levels.map((choice) => {
        const label = levelLabel(choice.level);
        const tone = levelTone(choice.level);
        const meaning = levelMeaning(choice.level);
        const options: DestinationOption[] = push.map((destination) => ({
          id: destination.id,
          label: destination.label,
          name: destinationCellLabel(label, destination.label),
          selected: isRouted(choice, destination.id),
          onToggle: (selected) =>
            onUpdate(
              "notifications",
              destinationCellUpdate(config, levels, choice.level, destination, selected),
            ),
        }));
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
              {meaning && <span className="text-[11px] text-[var(--ct-muted-fg)]">{meaning}</span>}
              {reachesNothing(choice, columns) && (
                <span className="text-[10px] text-[var(--ct-muted)]">panel only</span>
              )}
            </div>
            {local.map((destination) => (
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
            {push.length > 0 && (
              <DestinationPicker
                name={pushPickerLabel(label)}
                label={PUSH_DESTINATION_LABEL}
                summary={destinationSummary(push.filter((d) => isRouted(choice, d.id)))}
                options={options}
              />
            )}
          </div>
        );
      })}
      <p className="text-[10px] text-[var(--ct-muted)] leading-relaxed">
        Every level is kept in the panel; these choose what else it reaches. A message that names
        no level is kept too, and never reaches the system or any push channel — there is no
        fallback level and no way to widen this set from here.
      </p>
    </div>
  );
}
