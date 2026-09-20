// Run: npx tsx src/components/MeshPanel/webUiTarget.test.ts

import type { AppConfig } from "../../lib/types";
import type { ServeReconcileSnapshot, ServeStatus } from "../../lib/webAccess";
import { buildWebUiTarget, type NetworkInfo } from "./webUiTarget";

let passed = 0;
let failed = 0;

function assert(label: string, actual: unknown, expected: unknown) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected ${e}, got ${a}`);
    failed++;
  }
}

const config = {
  api: { port: 7685, token: "secret" },
} as AppConfig;

const network: NetworkInfo = {
  tailscale_ip: "100.64.10.3",
  local_ip: "192.168.1.20",
  api_port: 7685,
};

function readySnapshot(url = "https://rollacode-bot.example-tailnet.ts.net"): ServeReconcileSnapshot {
  return {
    state: "ready",
    url,
    api_port: 7685,
    desired_https_port: 443,
    chosen_https_port: 443,
    operator_bootstrap_required: false,
    bootstrap_command: null,
    binary: {
      installed: true,
      path: "/usr/bin/tailscale",
      cli_version: "1.80.0",
      daemon_version: "1.80.0",
      version_skew: false,
      version_output: null,
      error: null,
    },
    route: {
      host: "rollacode-bot.example-tailnet.ts.net",
      https_port: 443,
      proxy: "http://127.0.0.1:7685",
      ownership: "owned",
    },
    routes: [],
    conflict: null,
    health: {
      checked_url: `${url}/api/health`,
      ok: true,
      status: 200,
      daemon_build: "abc123",
      expected_build: "abc123",
      build_matches: true,
      error: null,
    },
  };
}

function serve(over: Partial<ServeStatus> = {}): ServeStatus {
  const snapshot = readySnapshot();
  return {
    installed: true,
    active: true,
    https_url: snapshot.url,
    self_dns: "rollacode-bot.example-tailnet.ts.net",
    platform: "linux",
    snapshot,
    ...over,
  };
}

console.log("buildWebUiTarget:");

const ready = buildWebUiTarget(config, network, serve());
assert(
  "ready serve status wins over raw tailscale IP",
  ready.link,
  "https://rollacode-bot.example-tailnet.ts.net/#token=secret",
);
assert("ready serve status is not fallback", ready.usingFallback, false);

const cachedServe = buildWebUiTarget(config, network, serve({
  snapshot: { ...readySnapshot(), state: "setup_required", url: null },
  https_url: "https://rollacode-bot.example-tailnet.ts.net",
  active: true,
}));
assert(
  "active serve https_url wins when snapshot is not ready",
  cachedServe.link,
  "https://rollacode-bot.example-tailnet.ts.net/#token=secret",
);
assert("active serve https_url is not fallback", cachedServe.usingFallback, false);

const fallback = buildWebUiTarget(config, network, null);
assert(
  "network tailscale IP remains fallback without serve",
  fallback.link,
  "http://100.64.10.3:7685/#token=secret",
);
assert("network IP is fallback", fallback.usingFallback, true);

if (failed > 0) {
  console.error(`\n${failed} webUiTarget tests failed`);
  process.exit(1);
}
console.log(`\n✓ ${passed} webUiTarget tests passed`);
