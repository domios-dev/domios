/** A chat block the helper may show; it renders through the same `@ds-<name>` registry as an agent reply. */
export type HelperBlock = { kind: "guide"; sectionId: string } | { kind: "plugin"; pluginId: string };

export interface HelperPluginFact {
  id: string;
  name: string;
  installed: boolean;
}

/** What the host knows right now; `null` means unknown, and unknown never counts as unused. */
export interface HelperFacts {
  telegramConnected: boolean | null;
  plugins: readonly HelperPluginFact[] | null;
  blueprintsInUse: boolean | null;
}

export const UNKNOWN_FACTS: HelperFacts = { telegramConnected: null, plugins: null, blueprintsInUse: null };

export const MESSENGER_PLUGIN_IDS = ["telegram-client", "teams-client"] as const;

export function blockSource(block: HelperBlock): string {
  return block.kind === "guide" ? `@ds-guide ${block.sectionId}` : `@ds-plugin ${block.pluginId}`;
}

const BLOCK_LINE = /^[\t ]*@ds-(guide|plugin)[\t ]+([a-z0-9][a-z0-9._-]*)[\t ]*$/i;

/** Standalone block lines of a reply, in order; inline mentions and other text are ignored. */
export function blocksInText(text: string): HelperBlock[] {
  return text.replace(/\r/g, "").split("\n").flatMap((line): HelperBlock[] => {
    const match = BLOCK_LINE.exec(line);
    if (!match) return [];
    const id = match[2].toLowerCase();
    return [match[1].toLowerCase() === "guide" ? { kind: "guide", sectionId: id } : { kind: "plugin", pluginId: id }];
  });
}

/** Whether the feature a block sets up is already in use: true, false, or null when the host cannot tell. */
export function featureInUse(block: HelperBlock, facts: HelperFacts): boolean | null {
  if (block.kind === "plugin") {
    if (!facts.plugins) return null;
    return facts.plugins.find((plugin) => plugin.id === block.pluginId)?.installed ?? null;
  }
  switch (block.sectionId) {
    case "phone":
      return facts.telegramConnected;
    case "messenger-plugins": {
      const listed = facts.plugins?.filter((plugin) => (MESSENGER_PLUGIN_IDS as readonly string[]).includes(plugin.id)) ?? [];
      return listed.length ? listed.some((plugin) => plugin.installed) : null;
    }
    case "blueprints":
      return facts.blueprintsInUse;
    default:
      return null;
  }
}

/** The agent's reply to the user's latest message, joined; null until it has said something. */
export function latestAnswerText(messages: readonly { type: string; content: string }[]): string | null {
  let start = messages.length;
  while (start > 0 && messages[start - 1].type !== "user") start--;
  const answer = messages.slice(start).filter((message) => message.type === "assistant").map((message) => message.content);
  return answer.length ? answer.join("\n") : null;
}
