import type { ChatWindowConfig, WatcherTickInput } from "@codeterm/plugin-sdk";

export interface EngineMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

const VERDICT_CONTRACT = [
  "Respond ONLY with a JSON object of this exact shape (no markdown fences, no surrounding prose):",
  "{",
  '  "status": "ok" | "attention" | "stalled",',
  '  "summary": "<one-line assessment>",',
  '  "state": <updated state object>,',
  '  "actions": [{ "kind": "nudge" | "notify" | "report", "tab": "<optional Tab id>", "message": "<text>" }]',
  "}",
].join("\n");

export function assembleChat(
  history: EngineMessage[],
  window?: ChatWindowConfig,
): EngineMessage[] {
  const maxMessages = window?.maxMessages;
  if (maxMessages === undefined) {
    return history;
  }

  const leadingSystem = history[0]?.role === "system" ? history[0] : undefined;
  const nonSystem = history.filter((m) => m.role !== "system");
  const tail = nonSystem.slice(-maxMessages);

  return leadingSystem ? [leadingSystem, ...tail] : tail;
}

export function assembleMachine(
  charter: string,
  state: unknown,
  input: WatcherTickInput | { query: string },
): EngineMessage[] {
  return [
    {
      role: "system",
      content: `${charter}\n\n${VERDICT_CONTRACT}`,
    },
    {
      role: "user",
      content: JSON.stringify({ state, input }),
    },
  ];
}
