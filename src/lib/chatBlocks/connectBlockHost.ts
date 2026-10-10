import type { BridgeConnectInfo, ConnectBlockHost, ConnectBlockState } from "@codeterm/chat-ui";

export type BridgeSnapshot<R> = { kind: "loading" } | { kind: "unavailable"; message: string } | { kind: "ready"; raw: R };

export interface MessengerBridge<R> {
  store: {
    get(): BridgeSnapshot<R>;
    subscribe(listener: () => void): () => void;
    refresh(): Promise<void>;
    connect(token?: string): Promise<void>;
    disconnect(): Promise<void>;
  };
  describe(raw: R): BridgeConnectInfo;
}

const UNSUPPORTED: ConnectBlockState = { kind: "unsupported" };
const LOADING: ConnectBlockState = { kind: "loading" };

export function connectBlockState<R>(snapshot: BridgeSnapshot<R>, describe: (raw: R) => BridgeConnectInfo): ConnectBlockState {
  switch (snapshot.kind) {
    case "loading":
      return LOADING;
    case "unavailable":
      return { kind: "unavailable", message: snapshot.message };
    case "ready":
      return { kind: "ready", bridge: describe(snapshot.raw) };
  }
}

/** Connect card services over the shared messenger stores the Messengers settings card also uses. */
export function createConnectBlockHost(bridges: Readonly<Record<string, MessengerBridge<unknown>>>, openSettings: (bridgeId: string) => void): ConnectBlockHost {
  const derived = new Map<string, { from: unknown; state: ConnectBlockState }>();
  const bridgeFor = (bridgeId: string) => (Object.prototype.hasOwnProperty.call(bridges, bridgeId) ? bridges[bridgeId] : undefined);
  const bridgeOrThrow = (bridgeId: string) => {
    const bridge = bridgeFor(bridgeId);
    if (!bridge) throw new Error(`Domios has no "${bridgeId}" messenger`);
    return bridge;
  };
  return {
    binding: {
      subscribe(listener) {
        const stops = Object.values(bridges).map((bridge) => bridge.store.subscribe(listener));
        return () => {
          for (const stop of stops) stop();
        };
      },
      read(bridgeId) {
        const bridge = bridgeFor(bridgeId);
        if (!bridge) return UNSUPPORTED;
        const snapshot = bridge.store.get();
        const cached = derived.get(bridgeId);
        if (cached?.from === snapshot) return cached.state;
        const state = connectBlockState(snapshot, bridge.describe);
        derived.set(bridgeId, { from: snapshot, state });
        return state;
      },
      refresh(bridgeId) {
        void bridgeFor(bridgeId)?.store.refresh();
      },
    },
    connect: async (bridgeId, token) => bridgeOrThrow(bridgeId).store.connect(token),
    disconnect: async (bridgeId) => bridgeOrThrow(bridgeId).store.disconnect(),
    openSettings,
  };
}
