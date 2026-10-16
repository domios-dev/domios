import type { ServeReconcileSnapshot, ServeReconcileState } from "../../lib/webAccess";
export function webUiSnapshot(state: ServeReconcileState, over: Partial<ServeReconcileSnapshot> = {}): ServeReconcileSnapshot {
  return {
    state,
    url: state === "ready" ? "https://host.tail.ts.net/" : null,
    api_port: 7685,
    desired_https_port: 443,
    chosen_https_port: state === "ready" ? 443 : null,
    operator_bootstrap_required: state === "operator_bootstrap_required",
    bootstrap_command:
      state === "operator_bootstrap_required" ? "sudo tailscale set --operator=rollacode" : null,
    binary: {
      installed: state !== "setup_required",
      path: "/usr/bin/tailscale",
      cli_version: "1.80.0",
      daemon_version: "1.80.0",
      version_skew: false,
      version_output: null,
      error: null,
    },
    route:
      state === "ready"
        ? { host: "host.tail.ts.net", https_port: 443, proxy: "http://127.0.0.1:7685", ownership: "owned" }
        : null,
    routes:
      state === "ready"
        ? [{ host: "host.tail.ts.net", https_port: 443, proxy: "http://127.0.0.1:7685", ownership: "owned" }]
        : [],
    conflict: state === "conflict" ? "Port 443 already proxies http://127.0.0.1:9999." : null,
    health: {
      checked_url: state === "ready" ? "https://host.tail.ts.net/api/health" : null,
      ok: state === "ready",
      status: state === "unhealthy" ? 502 : state === "ready" ? 200 : null,
      daemon_build: "abc123",
      expected_build: "abc123",
      build_matches: state === "ready" ? true : null,
      error: state === "unhealthy" ? "connection refused" : null,
    },
    ...over,
  };
}

