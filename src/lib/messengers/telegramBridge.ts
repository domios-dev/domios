import type { BridgeConnectInfo, BridgePhase } from "@codeterm/chat-ui";
import { daemonFetch, type DaemonApiConfig } from "../webApi";

/** `GET /api/bridge/state` for Telegram. Token values never travel here, only their presence. */
export interface TelegramBridgeRaw {
  active: boolean;
  config_enabled: boolean;
  token_present: boolean;
  has_secret_in_vault: boolean;
  secret_name: string;
  started_at?: string | null;
  last_poll_ok?: string | null;
  last_poll_error?: string | null;
  bot_id?: number | null;
}

export interface TelegramBridgeApi {
  state(): Promise<TelegramBridgeRaw>;
  saveToken(secretName: string, token: string): Promise<void>;
  start(replaceToken: boolean): Promise<void>;
  stop(): Promise<void>;
}

export const TELEGRAM_SECRET_NAME = "telegram_bot_token";

const TELEGRAM_TOKEN = /^\d{5,15}:[A-Za-z0-9_-]{30,}$/;

/** A helpful line for a pasted value that cannot be a BotFather token; null when it looks right. */
export function telegramTokenProblem(token: string): string | null {
  if (!token.trim()) return "paste the token @BotFather gave you";
  return TELEGRAM_TOKEN.test(token.trim()) ? null : "that is not a bot token from @BotFather (it looks like 123456789:AA…)";
}

export function telegramPhase(raw: TelegramBridgeRaw): BridgePhase {
  if (!raw.active) return "disconnected";
  if (raw.last_poll_ok && (!raw.started_at || raw.last_poll_ok >= raw.started_at) && !raw.last_poll_error) return "connected";
  return raw.last_poll_error ? "failing" : "connecting";
}

export function telegramConnectInfo(raw: TelegramBridgeRaw): BridgeConnectInfo {
  return {
    id: "telegram",
    name: "Telegram",
    description: "Message your General Agent from Telegram and get answers there.",
    phase: telegramPhase(raw),
    identity: raw.bot_id ? `bot ${raw.bot_id}` : null,
    tokenStored: raw.token_present || raw.has_secret_in_vault,
    error: raw.last_poll_error ?? null,
    method: {
      kind: "token",
      label: "Bot token",
      placeholder: "123456789:AA…",
      help: "Create a bot with @BotFather in Telegram and paste its token. It is saved to your secret store, never to a config file.",
    },
  };
}

/** A given token is checked and saved to the secret store first; the start then pulls it over any older token file. */
export async function connectTelegram(api: TelegramBridgeApi, token?: string): Promise<void> {
  if (token !== undefined) {
    const problem = telegramTokenProblem(token);
    if (problem) throw new Error(problem);
    await api.saveToken(TELEGRAM_SECRET_NAME, token.trim());
  }
  await api.start(token !== undefined);
}

async function failure(response: Response): Promise<Error> {
  const text = (await response.text()).trim();
  if (response.status === 412) return new Error("no bot token is saved yet");
  if (response.status === 409) return new Error("another Domios is already using this bot; disconnect it there first");
  return new Error(text || `HTTP ${response.status}`);
}

export function telegramBridgeApi(getApi: () => Promise<DaemonApiConfig>): TelegramBridgeApi {
  const call = async (path: string, init?: RequestInit) => {
    const response = await daemonFetch(await getApi(), path, init);
    if (!response.ok) throw await failure(response);
    return response;
  };
  const post = (path: string, body: unknown) => call(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return {
    async state() {
      return (await (await call("/api/bridge/state?service=telegram")).json()) as TelegramBridgeRaw;
    },
    async saveToken(secretName, token) {
      await post("/api/secrets/items", { name: secretName, value: token, notes: "Telegram bot bridge token", overwrite: true });
    },
    async start(replaceToken) {
      const result = (await (await post("/api/bridge/start", { service: "telegram", ensure_token: true, replace_token: replaceToken })).json()) as { ok?: boolean; error?: string };
      if (result.ok === false) throw new Error(result.error ?? "Telegram did not answer");
    },
    async stop() {
      await post("/api/bridge/stop", { service: "telegram" });
    },
  };
}
