import { configureDaemonTransport } from "../lib/daemonBootstrap";

export function configureDaemonFixture(token = "test-token"): void {
  const origin = /^https?:/.test(window.location.origin)
    ? window.location.origin : "http://127.0.0.1:7686";
  configureDaemonTransport({ origin, token });
}
