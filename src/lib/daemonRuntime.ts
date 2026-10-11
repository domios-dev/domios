import { baseDaemonTransport, daemonTransportForPeer, initializeDaemonTransport, onDaemonTransport } from "./daemonBootstrap";
import { relayTarget, webJson, localJson, invalidateWebRequestContext } from "./webApi";
import { bindOutputStreamTransport } from "./outputStream";
import { bindWebEventsTransport } from "./webEvents";
import { bindRemoteTabEventsTransport } from "./remoteTabEvents";
import { bindNativeBridgeTransport } from "./nativeBridge";
import { bindOrchestrationTransport } from "./transport/orchestrationRuntime";
import type { TransportClient } from "./transportClient";

export const selectedDaemonTransport: TransportClient = {
  json: (method, path, body, opts) => webJson(method, path, body, opts),
  localJson: (method, path, body, opts) => opts ? baseDaemonTransport().localJson(method, path, body, opts) : localJson(method, path, body),
  request: (path, init, local) => (local ? baseDaemonTransport() : daemonTransportForPeer(relayTarget()?.id)).request!(path, init, local),
  upload: (path, form, opts) => baseDaemonTransport().upload(path, form, opts),
  subscribe: (path, handler, opts) => baseDaemonTransport().subscribe(path, handler, opts),
  currentToken: () => baseDaemonTransport().currentToken(),
  origin: () => baseDaemonTransport().origin(),
};

let runtimeClient: TransportClient | undefined;
function bindDaemonRuntime(base: TransportClient): void {
  if (runtimeClient && runtimeClient !== base) invalidateWebRequestContext();
  runtimeClient = base;
  bindOutputStreamTransport(base);
  bindWebEventsTransport(base);
  bindRemoteTabEventsTransport(base);
  bindNativeBridgeTransport(base);
  bindOrchestrationTransport(selectedDaemonTransport);
}

let bound = false;
export function ensureDaemonRuntime(): void {
  if (!bound) { bound = true; onDaemonTransport(bindDaemonRuntime); }
  initializeDaemonTransport();
}
