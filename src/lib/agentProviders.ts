import type { ChatMessage } from "../components/ChatView/parser";
import { parseJsonlSession, parseTerminalOutput } from "../components/ChatView/parser";

/** How a provider handles image file sending */
export interface ImageSendStrategy {
  /** Method: 'clipboard-paste' = copy to clipboard + send Ctrl+V, 'path' = send file path as text */
  method: 'clipboard-paste' | 'path';
  /** For clipboard-paste: the keystroke to send after copying to clipboard */
  pasteKeystroke?: string;
}

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
  /** How to send images to this agent. Default: path-based. */
  imageSend?: ImageSendStrategy;
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
  // Claude Code title patterns:
  //   "claude" (startup), "✳ Claude Code" (splash), "⠐ Task name" (working spinner)
  // The spinner uses Braille dots (⠐⠂⠈⠄⠁⠠⠰⠘) as activity indicator prefix.
  titlePattern: /claude|^[⠁⠂⠄⠈⠐⠠⠰⠘⠃⠅⠑⠒⠔⠕⠡⠢⠤⠥⠨⠱⠲⠴⠸✳✻] /i,
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
    /[✱✳⏳]\s*\w+[…\.]{0,3}$/,         // Claude thinking status (✱ Nebulizing…, ✳ Thinking…, etc.)
    /Esc to cancel.*Tab to amend/s,    // Prompt footer (rendered via prompt card instead)
    /Enter to confirm.*Esc to cancel/s, // Prompt footer variant
  ],
  imageSend: {
    method: 'clipboard-paste',
    pasteKeystroke: '\x16',  // Ctrl+V — Claude Code reads clipboard on this signal
  },
  icon: "\u2733",  // ✳
  iconColor: "#f0883e",
});

// Register Codex provider
registerAgentProvider({
  id: "codex",
  displayName: "Codex",
  titlePattern: /codex/i,
  supportsChatView: false,
  supportsResume: true,
  noisePatterns: [
    /^[\s─━═]+$/,
  ],
  icon: "\u25C6",  // ◆
  iconColor: "#10a37f",
});

// Register OpenCode provider (sst/opencode)
registerAgentProvider({
  id: "opencode",
  displayName: "OpenCode",
  titlePattern: /opencode/i,
  supportsChatView: false,
  supportsResume: true,
  noisePatterns: [
    /^[\s─━═█▀▄▌▐]+$/,
  ],
  icon: "\u25C7",  // ◇
  iconColor: "#5eead4",
});
