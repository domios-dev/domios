import type { AgentCategoryChoice, AppConfig } from "../../lib/types";
import { Toggle } from "./shared";

export function setCategoryEnabled(
  current: AgentCategoryChoice[],
  category: string,
  enabled: boolean,
): AgentCategoryChoice[] {
  return current.map((choice) => (choice.category === category ? { ...choice, enabled } : choice));
}

export function categoryLabel(category: string): string {
  return category.charAt(0).toUpperCase() + category.slice(1);
}

/**
 * The desktop counterpart of the Telegram push gate.
 *
 * The rows come from the daemon's own closed category set, so this control
 * mirrors what can reach the user rather than restating it. An agent message
 * that names no category is not listed here because it cannot arrive at all.
 */
export function AgentNotificationCategories({
  config,
  onUpdate,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const categories = config.notifications?.agent_categories ?? [];
  if (categories.length === 0) return null;

  return (
    <div className="flex flex-col gap-2" data-testid="settings-agent-notification-categories">
      {categories.map((choice) => (
        <Toggle
          key={choice.category}
          label={`Alert me: ${categoryLabel(choice.category)}`}
          checked={choice.enabled}
          onChange={(value) =>
            onUpdate("notifications", {
              agent_categories: setCategoryEnabled(categories, choice.category, value),
            })
          }
        />
      ))}
      <p className="text-[10px] text-[var(--ct-muted)]">
        Only these categories can raise a desktop alert. Routine progress cannot: an agent message
        that fits none of them stays in the bell instead of interrupting you.
      </p>
    </div>
  );
}
