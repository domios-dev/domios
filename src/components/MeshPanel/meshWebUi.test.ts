import { webUiSnapshot as snap } from "./webUiFixture.testdata";
import { deriveWebUiView } from "./meshWebUi";
type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("deriveWebUiView: ready exposes Copy + Disable, no setup/repair", () => {
  const v = deriveWebUiView(snap("ready"), false);
  assert(v.state === "ready", "state ready");
  assert(Boolean(v.url), "ready url available to the Web UI surface");
  assert(v.canCopyUrl && v.canDisable, "copy + disable");
  assert(!v.canSetup && !v.canRepair, "no setup/repair when ready");
});

test("deriveWebUiView: setup_required offers setup only, no open/disable", () => {
  const v = deriveWebUiView(snap("setup_required"), false);
  assert(v.label === "Setup required", "setup label");
  assert(v.canSetup && !v.canCopyUrl, "setup, no published url");
  assert(!v.canDisable, "no owned route → no disable");
});

test("deriveWebUiView: operator bootstrap surfaces the exact command", () => {
  const v = deriveWebUiView(snap("operator_bootstrap_required"), false);
  assert(v.state === "operator_bootstrap_required", "state preserved");
  assert(v.operatorBootstrap, "operator flag");
  assert(v.detail?.includes("sudo tailscale set --operator=rollacode"), "command in detail");
});

test("deriveWebUiView: conflict + unhealthy offer repair, never open", () => {
  const c = deriveWebUiView(snap("conflict"), false);
  assert(c.canRepair && !c.canCopyUrl, "conflict repair, no published url");
  assert(c.detail?.includes("already proxies"), "conflict reason surfaced");
  const u = deriveWebUiView(snap("unhealthy"), false);
  assert(u.canRepair && !u.canCopyUrl, "unhealthy repair, no published url");
  assert(u.detail?.includes("connection refused"), "health error surfaced");
});

test("deriveWebUiView: loading before snapshot shows Checking, no actions", () => {
  const v = deriveWebUiView(null, true);
  assert(v.state === "loading" && v.label === "Checking…", "loading label");
  assert(!v.canCopyUrl && !v.canSetup && !v.canRepair && !v.canDisable, "no actions while loading");
});

test("deriveWebUiView: version skew is reported truthfully when ready", () => {
  const v = deriveWebUiView(
    snap("ready", { binary: { ...snap("ready").binary, cli_version: "1.80.0", daemon_version: "1.72.0", version_skew: true } }),
    false,
  );
  assert(v.detail?.includes("skew"), "skew detail present");
});


async function main() { for (const [name, fn] of tests) { await fn(); console.log("✓", name); } }
void main();
