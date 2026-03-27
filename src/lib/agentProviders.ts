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
  /** TUI noise — text matching any pattern is dropped from chat view */
  noisePatterns?: RegExp[];
  icon: string;
  iconColor: string;
}

/** Check if text is TUI noise for a given provider */
export function isProviderNoise(providerId: string | null | undefined, text: string): boolean {
  const provider = getAgentProvider(providerId);
  if (!provider?.noisePatterns) return false;
  const t = text.trim();
  if (!t) return true;
  return provider.noisePatterns.some(p => p.test(t));
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
  noisePatterns: [
    /bypass permissions on/i,
    /shift\+tab to cycle/i,
    /Update available!.*brew upgrade/i,
    /esc to interrupt/i,
    /\? for shortcuts/i,
    /ctrl\+g to edit/i,
    /Monthly spend limit/i,
    /hold Space/i,
    /^[\s─━═]+$/,
    /^👍$/,                    // Claude emoji reaction
    /^\*\s*(Wandering|Thinking|Pondering|Cogitating|Musing)/i,  // Claude thinking status
  ],
  icon: "\u2733",  // ✳
  iconColor: "#f0883e",
});
