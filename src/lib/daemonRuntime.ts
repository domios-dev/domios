import { baseDaemonTransport, daemonTransportForPeer, initializeDaemonTransport, onDaemonTransport } from "./daemonBootstrap";
import { relayTarget, webJson, localJson, invalidateWebRequestContext } from "./webApi";
import { bindOutputStreamTransport } from "./outputStream";
import { bindWebEventsTransport } from "./webEvents";
import { bindRemoteTabEventsTransport } from "./remoteTabEvents";
import { bindNativeBridgeTransport } from "./nativeBridge";
import { configureOrchestrationTransport } from "./transport/orchestrationRuntime";
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

onDaemonTransport((base) => {
  invalidateWebRequestContext();
  bindOutputStreamTransport(base);
  bindWebEventsTransport(base);
  bindRemoteTabEventsTransport(base);
  bindNativeBridgeTransport(base);
  configureOrchestrationTransport(selectedDaemonTransport);
});

export function ensureDaemonRuntime(): void { initializeDaemonTransport(); }
