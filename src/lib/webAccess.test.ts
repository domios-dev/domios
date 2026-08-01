import {
  buildWebAccessUrls,
  webLinkWithToken,
  webLinkWithTokenAndRelay,
  serveHint,
  planServeSetup,
  type ServeStatus,
} from "./webAccess";

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

console.log("buildWebAccessUrls priority:");

const full = buildWebAccessUrls(
  {
    tailscale_https_url: "https://host.ts.net",
    tailscale_ip: "100.64.0.5",
    local_ip: "192.168.1.10",
    api_port: 7685,
  },
  9999,
);
assert("https serve is primary", full.primary, "https://host.ts.net/");
assert("not fallback when https present", full.usingFallback, false);
assert("full ordered list", full.urls, [
  "https://host.ts.net/",
  "http://100.64.0.5:7685/",
  "http://192.168.1.10:7685/",
  "http://127.0.0.1:7685/",
]);

const noServe = buildWebAccessUrls(
  { tailscale_ip: "100.64.0.5", local_ip: "192.168.1.10", api_port: 7685 },
  9999,
);
assert("tailscale ip primary without serve", noServe.primary, "http://100.64.0.5:7685/");
assert("fallback flag set without serve", noServe.usingFallback, true);

const nonDefaultHttpsPort = buildWebAccessUrls(
  { tailscale_https_url: "https://host.ts.net:7600", api_port: 7685 },
  9999,
);
assert(
  "non-443 https port preserved in primary",
  nonDefaultHttpsPort.primary,
  "https://host.ts.net:7600/",
);

const lanOnly = buildWebAccessUrls({ local_ip: "192.168.1.10", api_port: 7685 }, 9999);
assert("lan ip primary", lanOnly.primary, "http://192.168.1.10:7685/");

const empty = buildWebAccessUrls(null, 7685);
assert("loopback is the floor", empty.primary, "http://127.0.0.1:7685/");
assert("loopback-only is fallback", empty.usingFallback, true);

assert("api_port from net overrides fallback", buildWebAccessUrls({ api_port: 1234 }, 9999).primary, "http://127.0.0.1:1234/");

const dup = buildWebAccessUrls({ local_ip: "127.0.0.1", api_port: 7685 }, 9999);
assert("dedups loopback collision", dup.urls, ["http://127.0.0.1:7685/"]);

console.log("\nwebLinkWithToken:");
assert(
  "appends token as fragment on https root",
  webLinkWithToken("https://host.ts.net/", "abc123"),
  "https://host.ts.net/#token=abc123",
);
assert(
  "encodes token",
  webLinkWithToken("http://127.0.0.1:7685/", "a b/c"),
  "http://127.0.0.1:7685/#token=a%20b%2Fc",
);
assert("no token → unchanged", webLinkWithToken("http://127.0.0.1:7685/", null), "http://127.0.0.1:7685/");
let rejectedUnsafeOrigin = false;
try {
  webLinkWithToken("https://evil.example.com/", "secret");
} catch {
  rejectedUnsafeOrigin = true;
}
assert("rejects token attachment to an untrusted origin", rejectedUnsafeOrigin, true);
assert(
  "builds authenticated self link",
  webLinkWithTokenAndRelay("https://host.ts.net/", "abc123", null),
  "https://host.ts.net/#token=abc123&relay=self",
);
assert(
  "builds authenticated relay link",
  webLinkWithTokenAndRelay("https://host.ts.net/", "abc123", { id: "peer/a" }),
  "https://host.ts.net/#token=abc123&relay=peer%2Fa",
);

console.log("\nserveHint:");
assert("hint includes port", serveHint(7685).includes("127.0.0.1:7685"), true);

console.log("\nplanServeSetup:");

const notInstalled = planServeSetup(
  { installed: false, active: false },
  7685,
  "linux",
);
assert("not-installed state when tailscale absent", notInstalled.state, "not-installed");
assert("not-installed cannot enable", notInstalled.canEnable, false);

const alreadyActive = planServeSetup(
  { installed: true, active: true, https_url: "https://host.ts.net", self_dns: "host.ts.net" },
  7685,
  "linux",
);
assert("active state when serve already targets us", alreadyActive.state, "active");
assert("active cannot enable", alreadyActive.canEnable, false);

const canEnableLinux: ServeStatus = {
  installed: true,
  active: false,
  self_dns: "host.ts.net",
  platform: "linux",
};
const planLinux = planServeSetup(canEnableLinux, 7685, "linux");
assert("can-enable when installed + inactive", planLinux.state, "can-enable");
assert("can-enable flag set", planLinux.canEnable, true);
assert("linux runs serve only (no cert step)", planLinux.commands, [
  "tailscale serve --bg --https=443 http://127.0.0.1:7685",
]);
assert("result url uses self dns at root", planLinux.resultUrl, "https://host.ts.net/");

const planMac = planServeSetup(
  { installed: true, active: false, self_dns: "mac.ts.net" },
  7685,
  "macos",
);
assert("macos prepends tailscale cert step", planMac.commands, [
  "tailscale cert mac.ts.net",
  "tailscale serve --bg --https=443 http://127.0.0.1:7685",
]);

const planMacNoDns = planServeSetup(
  { installed: true, active: false, self_dns: null },
  7685,
  "macos",
);
assert("macos omits cert step without self dns", planMacNoDns.commands, [
  "tailscale serve --bg --https=443 http://127.0.0.1:7685",
]);
assert("no result url without self dns", planMacNoDns.resultUrl, null);

// Port-collision: api_port == the default public https port.
const collisionSelf = planServeSetup({ installed: true, active: false }, 443, "linux");
assert("port-collision when api_port is 443", collisionSelf.state, "port-collision");
assert("port-collision cannot enable", collisionSelf.canEnable, false);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
