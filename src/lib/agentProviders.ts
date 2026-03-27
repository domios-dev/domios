import type { ChatMessage } from "../components/ChatView/parser";
import { parseJsonlSession, parseTerminalOutput } from "../components/ChatView/parser";

export interface AgentProviderConfig {
  id: string;
  displayName: string;
  titlePattern: RegExp;
  supportsChatView: boolean;
  supportsResume: boolean;
  parseSessionData?: (data: string) => ChatMessage[];
  parseTerminalOutput?: (text: string) => ChatMessage[];
  icon: string;
  iconColor: string;
}

const providers = new Map<string, AgentProviderConfig>();

export function registerAgentProvider(config: AgentProviderConfig) {
  providers.set(config.id, config);
}

export function getAgentProvider(id: string | null | undefined): AgentProviderConfig | undefined {
  if (!id) return undefined;
  return providers.get(id);
}

export function detectAgentFromTitle(title: string): string | undefined {
  for (const [id, config] of providers) {
    if (config.titlePattern.test(title)) return id;
  }
  return undefined;
}

export function getAllProviders(): AgentProviderConfig[] {
  return Array.from(providers.values());
}

// Register Claude provider
registerAgentProvider({
  id: "claude",
  displayName: "Claude Code",
  titlePattern: /claude/i,
  supportsChatView: true,
  supportsResume: true,
  parseSessionData: parseJsonlSession,
  parseTerminalOutput: parseTerminalOutput,
  icon: "\u2733",  // ✳
  iconColor: "#f0883e",
});
