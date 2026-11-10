import type { PluginSnapshot } from "../components/Extensions/types";
import { createConsumerFlight } from "./consumerFlight";
import type { TransportClient } from "./transportClient";

type SnapshotClient = Pick<TransportClient, "json" | "localJson" | "origin" | "currentToken">;
const reads = createConsumerFlight<PluginSnapshot>();

export function readPluginSnapshot(client: SnapshotClient, options: { local?: boolean; peerId?: () => string | null; signal?: AbortSignal } = {}) {
  const origin = client.origin();
  const token = client.currentToken();
  const peerId = options.local ? null : options.peerId?.() ?? null;
  return reads.run(JSON.stringify([origin, token, peerId]), (signal) => {
    if (client.origin() !== origin || client.currentToken() !== token || (!options.local && (options.peerId?.() ?? null) !== peerId)) {
      throw new DOMException("Plugin snapshot context changed", "AbortError");
    }
    return options.local
      ? client.localJson<PluginSnapshot>("GET", "/api/plugins/snapshot", undefined, { signal })
      : client.json<PluginSnapshot>("GET", "/api/plugins/snapshot", undefined, { signal });
  }, options.signal);
}
