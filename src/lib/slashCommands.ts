export interface SlashCommandSuggestion {
  name: string;
  description: string;
}

export const CLAUDE_SLASH_COMMANDS: SlashCommandSuggestion[] = [
  { name: "/model",   description: "Switch model for this session" },
  { name: "/compact", description: "Compact the conversation context" },
  { name: "/clear",   description: "Clear conversation" },
  { name: "/agents",  description: "Manage agents" },
  { name: "/skills",  description: "Browse skills" },
  { name: "/config",  description: "Open config" },
  { name: "/help",    description: "Show help" },
  { name: "/add-dir", description: "Add a working directory" },
  { name: "/exit",    description: "Exit claude" },
  { name: "/login",   description: "Log in / refresh credentials" },
  { name: "/cost",    description: "Show token cost so far" },
  { name: "/status",  description: "Show status" },
];

export function filterSlashCommands(prefix: string): SlashCommandSuggestion[] {
  if (!prefix.startsWith("/")) return [];
  const needle = prefix.toLowerCase();
  return CLAUDE_SLASH_COMMANDS.filter((c) =>
    c.name.toLowerCase().startsWith(needle),
  );
}
