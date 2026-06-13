import { buildWebAccessUrls, webLinkWithToken, serveHint } from "./webAccess";

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

// ── priority chain ────────────────────────────────────────────────
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
assert("https serve is primary", full.primary, "https://host.ts.net/web/");
assert("not fallback when https present", full.usingFallback, false);
assert("full ordered list", full.urls, [
  "https://host.ts.net/web/",
  "http://100.64.0.5:7685/web/",
  "http://192.168.1.10:7685/web/",
  "http://127.0.0.1:7685/web/",
]);

const noServe = buildWebAccessUrls(
  { tailscale_ip: "100.64.0.5", local_ip: "192.168.1.10", api_port: 7685 },
  9999,
);
assert("tailscale ip primary without serve", noServe.primary, "http://100.64.0.5:7685/web/");
assert("fallback flag set without serve", noServe.usingFallback, true);

const nonDefaultHttpsPort = buildWebAccessUrls(
  { tailscale_https_url: "https://host.ts.net:7600", api_port: 7685 },
  9999,
);
assert(
  "non-443 https port preserved in primary",
  nonDefaultHttpsPort.primary,
  "https://host.ts.net:7600/web/",
);

const lanOnly = buildWebAccessUrls({ local_ip: "192.168.1.10", api_port: 7685 }, 9999);
assert("lan ip primary", lanOnly.primary, "http://192.168.1.10:7685/web/");

const empty = buildWebAccessUrls(null, 7685);
assert("loopback is the floor", empty.primary, "http://127.0.0.1:7685/web/");
assert("loopback-only is fallback", empty.usingFallback, true);

assert("api_port from net overrides fallback", buildWebAccessUrls({ api_port: 1234 }, 9999).primary, "http://127.0.0.1:1234/web/");

// dedup: loopback already implied; ensure no dupes when ips coincide
const dup = buildWebAccessUrls({ local_ip: "127.0.0.1", api_port: 7685 }, 9999);
assert("dedups loopback collision", dup.urls, ["http://127.0.0.1:7685/web/"]);

// ── token fragment ────────────────────────────────────────────────
console.log("\nwebLinkWithToken:");
assert(
  "appends token as fragment on https root",
  webLinkWithToken("https://host.ts.net/web/", "abc123"),
  "https://host.ts.net/web/#token=abc123",
);
assert(
  "encodes token",
  webLinkWithToken("http://127.0.0.1:7685/web/", "a b/c"),
  "http://127.0.0.1:7685/web/#token=a%20b%2Fc",
);
assert("no token → unchanged", webLinkWithToken("http://127.0.0.1:7685/web/", null), "http://127.0.0.1:7685/web/");

// ── hint carries the live port ────────────────────────────────────
console.log("\nserveHint:");
assert("hint includes port", serveHint(7685).includes("127.0.0.1:7685"), true);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
