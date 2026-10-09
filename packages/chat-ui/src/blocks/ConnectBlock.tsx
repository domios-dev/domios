import { useId, useRef, useState } from "react";
import { KeyRound, MessageCircle, Unplug } from "lucide-react";
import { BlockNotice } from "./BlockNotice";
import { BLOCK_INPUT, ChatBlockFrame, type ChatBlockAction } from "./ChatBlockFrame";
import { useChatBlockHost, type ConnectBlockHost } from "./ChatBlockHost";
import { useLiveBinding } from "./liveBinding";
import { connectResult, connectStatus, needsTokenEntry, type BridgeConnectInfo, type ConnectBlockState } from "./connectBlockModel";

const LOADING: ConnectBlockState = { kind: "loading" };

/** `@ds-connect <bridge-id>`: a messenger bridge's live status with its connect flow in place. */
export function ConnectBlock({ bridgeId, source }: { bridgeId: string; source: string }) {
  const host = useChatBlockHost().connect;
  const state = useLiveBinding(host?.binding, bridgeId, LOADING);
  if (!host) return <ConnectNotice source={source} bridgeId={bridgeId} text="Messenger connections are unavailable in this view." />;
  switch (state.kind) {
    case "loading":
      return <ConnectNotice source={source} bridgeId={bridgeId} text="Checking the connection…" />;
    case "unavailable":
      return <ConnectNotice source={source} bridgeId={bridgeId} text={`Connection status unavailable: ${state.message}`} />;
    case "unsupported":
      return <ConnectNotice source={source} bridgeId={bridgeId} text={`Domios has no “${bridgeId}” messenger to connect.`} unsupported />;
    case "ready":
      return <ConnectCard bridge={state.bridge} host={host} source={source} />;
  }
}

function ConnectCard({ bridge, host, source }: { bridge: BridgeConnectInfo; host: ConnectBlockHost; source: string }) {
  const [replacing, setReplacing] = useState(false);
  const [hasDraft, setHasDraft] = useState(false);
  const tokenRef = useRef<HTMLInputElement>(null);
  const fieldId = useId();
  const helpId = useId();
  const entering = needsTokenEntry(bridge, replacing);

  const actions: ChatBlockAction[] = [];
  if (bridge.phase === "disconnected") {
    actions.push({
      id: "connect",
      label: "Connect",
      busyLabel: "Connecting…",
      primary: true,
      disabled: entering && !hasDraft,
      reason: entering && !hasDraft ? `Paste the ${bridge.method.label.toLowerCase()} first` : undefined,
      run: async () => {
        const token = entering ? tokenRef.current?.value.trim() : undefined;
        if (entering && !token) throw new Error(`the ${bridge.method.label.toLowerCase()} is empty`);
        await host.connect(bridge.id, token);
        if (tokenRef.current) tokenRef.current.value = "";
        setHasDraft(false);
        setReplacing(false);
        return connectResult(host.binding.read(bridge.id));
      },
    });
  }
  if (bridge.phase !== "connected" && bridge.tokenStored) {
    actions.push({
      id: "replace-token",
      label: replacing ? "Keep saved token" : "Use a different token",
      run: () => {
        setReplacing((open) => !open);
        setHasDraft(false);
      },
    });
  }
  if (bridge.phase !== "disconnected") {
    actions.push({
      id: "disconnect",
      label: "Disconnect",
      busyLabel: "Disconnecting…",
      confirm: {
        message: `Disconnect ${bridge.name}? Your General Agent stops answering there. The token stays saved.`,
        confirmLabel: "Disconnect",
      },
      run: async () => {
        await host.disconnect(bridge.id);
        return `Disconnected from ${bridge.name}.`;
      },
    });
  }

  const summary = bridge.phase === "connected"
    ? (bridge.identity ? `Connected as ${bridge.identity}.` : "Connected.")
    : bridge.phase === "failing" && bridge.error ? bridge.error : bridge.description;

  return (
    <ChatBlockFrame
      name="connect"
      source={source}
      title={bridge.name}
      icon={<MessageCircle size={14} aria-hidden="true" />}
      status={connectStatus(bridge.phase)}
      summary={summary}
      body={entering ? (
        <div className="flex min-w-0 flex-col gap-1">
          <label htmlFor={fieldId} className="text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>{bridge.method.label}</label>
          <div className="flex min-w-0 items-center gap-2">
            <KeyRound size={13} aria-hidden="true" className="shrink-0" style={{ color: "var(--ct-muted-fg)" }} />
            <input
              ref={tokenRef}
              id={fieldId}
              type="password"
              autoComplete="off"
              spellCheck={false}
              data-testid="chat-connect-token"
              aria-describedby={helpId}
              placeholder={bridge.method.placeholder}
              onInput={(event) => setHasDraft(event.currentTarget.value.trim().length > 0)}
              className={`${BLOCK_INPUT} flex-1 font-mono`}
            />
          </div>
          <p id={helpId} className="m-0 text-[11px] leading-relaxed" style={{ color: "var(--ct-muted-fg)" }}>{bridge.method.help}</p>
        </div>
      ) : undefined}
      details={(
        <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
          <dt>Bridge</dt><dd className="m-0 font-mono">{bridge.id}</dd>
          {bridge.identity && <><dt>Acts as</dt><dd className="m-0">{bridge.identity}</dd></>}
          <dt>Token</dt><dd className="m-0">{bridge.tokenStored ? "Saved on this machine" : "Not saved yet"}</dd>
          {bridge.error && <><dt>Last error</dt><dd className="m-0">{bridge.error}</dd></>}
        </dl>
      )}
      actions={actions}
      fullPage={{ label: "Open Messengers settings", open: () => host.openSettings(bridge.id) }}
      askText={`About the ${bridge.name} connection (\`${source}\`): `}
    />
  );
}

function ConnectNotice({ source, bridgeId, text, unsupported = false }: { source: string; bridgeId: string; text: string; unsupported?: boolean }) {
  return <BlockNotice name="connect" source={source} label={`Connect ${bridgeId}`} text={text} icon={MessageCircle} missing={unsupported} missingIcon={Unplug} />;
}
