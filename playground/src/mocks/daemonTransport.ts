import { createTransportClient, type TransportOptions } from "../../../src/lib/transportClient";
import { mockDaemonFetch } from "./backend";

export function withStreamMux(options: TransportOptions) {
  return createTransportClient({ ...options, fetch: mockDaemonFetch });
}
