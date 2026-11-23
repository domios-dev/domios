import { createTransportClient, type TransportOptions } from "../../../../src/lib/transportClient";

/** The film's daemon transport: plain fetches that the film's fixture routes answer (no stream mux). */
export function withStreamMux(options: TransportOptions) {
  return createTransportClient(options);
}
