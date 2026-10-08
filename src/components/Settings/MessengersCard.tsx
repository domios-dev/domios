import { useId, useRef, useState, type FormEvent } from "react";
import { MessageCircle } from "lucide-react";
import { connectStatus, needsTokenEntry, type BridgeConnectInfo, type BridgePhase } from "@codeterm/chat-ui";
import type { AppConfig } from "../../lib/types";
import { telegramConnectInfo } from "../../lib/messengers/telegramBridge";
import { telegramBridgeStore, useTelegramBridge, type TelegramBridgeStore } from "../../lib/messengers/telegramBridgeStore";
import { Button } from "../shared/Button";
import { FormRow } from "../shared/FormRow";
import { UI_INPUT } from "../shared/design";
import { SettingsCard, SettingsPill } from "./shared";

export function parseAllowedUsers(text: string): string[] {
  return text.split(",").map((value) => value.trim()).filter(Boolean);
}

const PILL_TONE: Record<BridgePhase, "green" | "accent" | "amber" | "muted"> = {
  connected: "green",
  connecting: "accent",
  failing: "amber",
  disconnected: "muted",
};

export function MessengersCard({
  config,
  onUpdate,
  store = telegramBridgeStore,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  store?: TelegramBridgeStore;
}) {
  const usersId = useId();
  const snapshot = useTelegramBridge(store);
  const bridge = snapshot.kind === "ready" ? telegramConnectInfo(snapshot.raw) : null;
  const [allowedUsersText, setAllowedUsersText] = useState(
    (config.integrations?.telegram_allowed_users ?? []).join(", "),
  );

  return (
    <SettingsCard
      icon={<MessageCircle size={16} />}
      title="Messengers"
      headerRight={bridge ? <SettingsPill tone={PILL_TONE[bridge.phase]}>Telegram: {connectStatus(bridge.phase).label.toLowerCase()}</SettingsPill> : undefined}
      description="Talk to your General Agent from a chat app. Replies go back to the chat you wrote from."
    >
      {snapshot.kind === "loading" && <p className="m-0 text-xs text-[var(--ct-muted-fg)]">Checking the Telegram bridge…</p>}
      {snapshot.kind === "unavailable" && <p className="m-0 text-xs text-[var(--ct-muted-fg)]">Telegram bridge status unavailable: {snapshot.message}</p>}
      {bridge && <TelegramConnection bridge={bridge} store={store} />}
      <FormRow label="Allowed user IDs" htmlFor={usersId} hint="Comma-separated Telegram user IDs. Empty allows anyone who finds the bot.">
        <input
          id={usersId}
          type="text"
          data-testid="settings-telegram-allowed-users"
          value={allowedUsersText}
          onChange={(event) => {
            setAllowedUsersText(event.target.value);
            onUpdate("integrations", { telegram_allowed_users: parseAllowedUsers(event.target.value) });
          }}
          placeholder="123456789, 987654321"
          className={UI_INPUT.fullMono}
        />
      </FormRow>
    </SettingsCard>
  );
}

type Outcome = { kind: "idle" } | { kind: "busy" } | { kind: "failed"; message: string };

function TelegramConnection({ bridge, store }: { bridge: BridgeConnectInfo; store: TelegramBridgeStore }) {
  const tokenId = useId();
  const token = useRef<HTMLInputElement>(null);
  const [replacing, setReplacing] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [outcome, setOutcome] = useState<Outcome>({ kind: "idle" });
  const busy = outcome.kind === "busy";
  const entering = needsTokenEntry(bridge, replacing);

  const run = async (action: () => Promise<void>) => {
    if (busy) return;
    setOutcome({ kind: "busy" });
    try {
      await action();
      setOutcome({ kind: "idle" });
      setReplacing(false);
      setConfirming(false);
    } catch (error) {
      setOutcome({ kind: "failed", message: error instanceof Error ? error.message : String(error) });
    }
  };
  const connect = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const value = entering ? token.current?.value ?? "" : undefined;
    if (token.current) token.current.value = "";
    void run(() => store.connect(value));
  };

  return (
    <div className="space-y-2" data-testid="settings-telegram-connection" data-phase={bridge.phase}>
      {bridge.phase === "disconnected" ? (
        <form onSubmit={connect} className="space-y-2">
          {entering ? (
            <FormRow label={bridge.method.label} htmlFor={tokenId} hint={bridge.method.help}>
              <input
                id={tokenId}
                ref={token}
                type="password"
                autoComplete="off"
                spellCheck={false}
                required
                data-testid="settings-telegram-token"
                placeholder={bridge.method.placeholder}
                className={UI_INPUT.fullMono}
              />
            </FormRow>
          ) : (
            <p className="m-0 text-xs text-[var(--ct-muted-fg)]">A bot token is saved on this machine.</p>
          )}
          <div className="flex flex-wrap items-center gap-2">
            <Button type="submit" variant="primary" size="sm" disabled={busy} data-testid="settings-telegram-connect">
              {busy ? "Connecting…" : "Connect Telegram"}
            </Button>
            {bridge.tokenStored && (
              <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => setReplacing((open) => !open)}>
                {replacing ? "Keep saved token" : "Use a different token"}
              </Button>
            )}
          </div>
        </form>
      ) : (
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <span className="min-w-0 flex-1 text-[var(--ct-fg)]">
            {bridge.phase === "connected" && (bridge.identity ? `Connected as ${bridge.identity}.` : "Connected.")}
            {bridge.phase === "connecting" && "Starting. Waiting for Telegram to answer."}
            {bridge.phase === "failing" && `Telegram is not answering${bridge.error ? `: ${bridge.error}` : "."}`}
          </span>
          {confirming ? (
            <>
              <span className="text-[var(--ct-muted-fg)]">Your General Agent stops answering in Telegram. The token stays saved.</span>
              <Button type="button" variant="danger" size="sm" disabled={busy} data-testid="settings-telegram-disconnect-confirm" onClick={() => void run(() => store.disconnect())}>
                Disconnect
              </Button>
              <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => setConfirming(false)}>Cancel</Button>
            </>
          ) : (
            <Button type="button" variant="ghost" size="sm" disabled={busy} data-testid="settings-telegram-disconnect" onClick={() => setConfirming(true)}>
              Disconnect
            </Button>
          )}
        </div>
      )}
      <div aria-live="polite" className="empty:hidden text-xs">
        {outcome.kind === "failed" && <span role="alert" className="text-[var(--ct-red)]">Telegram: {outcome.message}</span>}
      </div>
    </div>
  );
}
