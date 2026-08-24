import type { AgentLevelChoice, AppConfig } from "../../lib/types";
import { Toggle } from "./shared";

export function setLevelEnabled(
  current: AgentLevelChoice[],
  level: string,
  enabled: boolean,
): AgentLevelChoice[] {
  return current.map((choice) => (choice.level === level ? { ...choice, enabled } : choice));
}

/** Derived from the wire value, so no surface restates the level set. */
export function levelLabel(level: string): string {
  const spaced = level.replace(/_/g, " ");
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

/**
 * The desktop counterpart of the Telegram push gate.
 *
 * The rows come from the daemon's own closed level set, so this control
 * mirrors what can reach the user rather than restating it. An agent message
 * that names no level is not listed here because it cannot arrive at all.
 */
export function AgentNotificationLevels({
  config,
  onUpdate,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const levels = config.notifications?.agent_levels ?? [];
  if (levels.length === 0) return null;

  return (
    <div className="flex flex-col gap-2" data-testid="settings-agent-notification-levels">
      {levels.map((choice) => (
        <Toggle
          key={choice.level}
          label={`Alert me: ${levelLabel(choice.level)}`}
          checked={choice.enabled}
          onChange={(value) =>
            onUpdate("notifications", {
              agent_levels: setLevelEnabled(levels, choice.level, value),
            })
          }
        />
      ))}
      <p className="text-[10px] text-[var(--ct-muted)]">
        Only these levels can raise a desktop alert. An agent message whose level is switched off
        stays in the bell instead of interrupting you.
      </p>
    </div>
  );
}
