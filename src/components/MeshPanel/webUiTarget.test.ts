// Run: npx tsx src/components/MeshPanel/webUiTarget.test.ts

import type { AppConfig } from "../../lib/types";
import type { ServeReconcileSnapshot, ServeStatus } from "../../lib/webAccess";
import { buildWebUiTarget, classifyWebUiRoute, displayedWebUrl, shouldShowWebSetupHelper, webRouteLabel, webUiListenerReady, type NetworkInfo } from "./webUiTarget";

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
  tailscale_ip: "100.110.3.12",
  local_ip: "192.168.1.20",
  api_port: 7685,
};

function readySnapshot(url = "https://rollacode-bot.tail0e459c.ts.net"): ServeReconcileSnapshot {
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
      host: "rollacode-bot.tail0e459c.ts.net",
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
    self_dns: "rollacode-bot.tail0e459c.ts.net",
    platform: "linux",
    snapshot,
    ...over,
  };
}

assert("ready HTTPS access needs no setup helper", shouldShowWebSetupHelper("tailscale_https", true), false);
assert("unready HTTPS access retains setup helper", shouldShowWebSetupHelper("tailscale_https", false), true);
assert("LAN access retains setup helper", shouldShowWebSetupHelper("lan", true), true);

assert("route summary names HTTPS and Tailscale", webRouteLabel("tailscale_https"), "HTTPS · Tailscale");
assert("route summary distinguishes local network", webRouteLabel("lan"), "Local network");


assert("detected IP alone is not a ready listener", webUiListenerReady(network, null), false);
assert("bound signed-out listener is ready without Serve", webUiListenerReady({ ...network, web_listener: "100.64.1.2:7698" }, null), true);
assert("unhealthy Serve is not ready", webUiListenerReady(network, serve({ snapshot: { ...readySnapshot(), state: "unhealthy" } })), false);
assert("ready listener supplies its actual port", buildWebUiTarget(config, { ...network, web_listener: "100.64.1.2:7698" }).link, "http://100.64.1.2:7698/#token=secret");
assert("ready IPv6 listener supports token access", buildWebUiTarget(config, { web_listener: "[fd7a:115c:a1e0::1]:7698" }).link, "http://[fd7a:115c:a1e0::1]:7698/#token=secret");
console.log("buildWebUiTarget:");

const ready = buildWebUiTarget(config, network, serve());
assert(
  "ready serve status wins over raw tailscale IP",
  ready.link,
  "https://rollacode-bot.tail0e459c.ts.net/#token=secret",
);
assert("ready serve status is not fallback", ready.usingFallback, false);

const cachedServe = buildWebUiTarget(config, network, serve({
  snapshot: { ...readySnapshot(), state: "setup_required", url: null },
  https_url: "https://rollacode-bot.tail0e459c.ts.net",
  active: true,
}));
assert(
  "unproven serve status falls back",
  cachedServe.link,
  "http://100.110.3.12:7685/#token=secret",
);
assert("unproven serve status is fallback", cachedServe.usingFallback, true);

const unhealthyServe = buildWebUiTarget(config, network, serve({
  snapshot: { ...readySnapshot(), state: "unhealthy", url: null },
  https_url: "https://rollacode-bot.tail0e459c.ts.net",
  active: true,
}));
assert(
  "unhealthy owned serve route falls back instead of offering its https_url",
  unhealthyServe.link,
  "http://100.110.3.12:7685/#token=secret",
);
assert("unhealthy serve route is fallback", unhealthyServe.usingFallback, true);

const fallback = buildWebUiTarget(config, network, null);
assert(
  "network tailscale IP remains fallback without serve",
  fallback.link,
  "http://100.110.3.12:7685/#token=secret",
);
assert("network IP is fallback", fallback.usingFallback, true);

for (const [net, status, kind, expectedHost] of [
  [network, serve(), "tailscale_https", "rollacode-bot.tail0e459c.ts.net"],
  [network, null, "tailscale_ip", "100.110.3.12"],
  [{ local_ip: "192.168.1.20" }, null, "lan", "192.168.1.20"],
  [null, null, "loopback", "127.0.0.1"],
  [{ local_ip: "localhost" }, null, "loopback", "localhost"],
  [network, serve({ active: true, snapshot: { ...readySnapshot(), state: "unhealthy", url: null } }), "tailscale_ip", "100.110.3.12"],
] as const) {
  const target = buildWebUiTarget(config, net, status);
  const route = classifyWebUiRoute(config, net, status);
  assert("route agrees with selected target", [route.kind, new URL(target.link).hostname], [kind, expectedHost]);
  assert("display URL has no token", displayedWebUrl(target.link).includes("secret"), false);
  assert("authenticated link keeps token", new URL(target.link).hash.includes("token=secret"), true);
}
assert("display strips token query too", displayedWebUrl("http://localhost:7686/?token=secret#token=secret"), "http://localhost:7686/");
assert("loopback explains phone limitation", classifyWebUiRoute(config, null).reason.includes("phone"), true);

if (failed > 0) {
  console.error(`\n${failed} webUiTarget tests failed`);
  process.exit(1);
}
console.log(`\n✓ ${passed} webUiTarget tests passed`);
