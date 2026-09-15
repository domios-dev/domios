import { invoke } from "../../lib/bridge";

export type TelegramTopicsState = "available" | "unavailable" | "unknown";

export interface TelegramThreadMenuState {
  bridgeActive: boolean;
  topics: TelegramTopicsState;
  bound: boolean;
  link: string | null;
}

interface WireThreadState {
  bridge_active?: boolean;
  topics?: string;
  bound?: boolean;
  link?: string | null;
}

export const TELEGRAM_THREAD_LABELS = {
  open: "Attach Telegram",
  close: "Detach Telegram",
  bridgeOff: "Start the Telegram bridge to open a thread.",
  topicsDisabled: "Enable topics for the bot in BotFather to talk to agents in threads.",
} as const;

export function telegramThreadMenuState(wire: WireThreadState | null | undefined): TelegramThreadMenuState {
  const raw = wire?.topics;
  const topics: TelegramTopicsState = raw === "available" || raw === "unavailable" ? raw : "unknown";
  return {
    bridgeActive: wire?.bridge_active === true,
    topics,
    bound: wire?.bound === true,
    link: typeof wire?.link === "string" ? wire.link : null,
  };
}

/** Why Open cannot succeed right now, or `null` when it can. */
export function telegramThreadHint(state: TelegramThreadMenuState): string | null {
  if (!state.bridgeActive) return TELEGRAM_THREAD_LABELS.bridgeOff;
  if (state.topics === "unavailable") return TELEGRAM_THREAD_LABELS.topicsDisabled;
  return null;
}

export async function fetchTelegramThreadState(tabId: string): Promise<TelegramThreadMenuState> {
  return telegramThreadMenuState(await invoke<WireThreadState>("bridge_thread_state", { tabId }));
}

export async function openTelegramThread(tabId: string): Promise<string | null> {
  const opened = await invoke<{ link?: string | null }>("bridge_thread_open", { tabId });
  return typeof opened?.link === "string" ? opened.link : null;
}

export async function closeTelegramThread(tabId: string): Promise<void> {
  await invoke("bridge_thread_close", { tabId });
}
