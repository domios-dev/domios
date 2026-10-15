import { DEV_BUILD } from "./devBuild";

export const EVENT_STREAM_READY = "codeterm-event-stream-ready";
let version = 0;
let firstConnectedAt: number | undefined;
let firstCatalogueAt: number | undefined;

export function eventStreamVersion(): number { return version; }

export function signalEventStreamReady(): void {
  firstConnectedAt ??= performance.now();
  version++;
  window.dispatchEvent(new Event(EVENT_STREAM_READY));
}

export function recordProviderCatalogueResponse(providerCount: number): void {
  if (!DEV_BUILD || firstConnectedAt === undefined || firstCatalogueAt !== undefined) return;
  firstCatalogueAt = performance.now();
  console.debug(`[provider-catalogue] first response ${(firstCatalogueAt - firstConnectedAt).toFixed(0)}ms after event-stream ready (${providerCount} providers)`);
  performance.measure("provider-catalogue:first-response", { start: firstConnectedAt, end: firstCatalogueAt });
}
