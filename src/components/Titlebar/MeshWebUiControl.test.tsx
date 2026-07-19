// Run: npx tsx src/components/Titlebar/MeshWebUiControl.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { ServeReconcileSnapshot, ServeReconcileState } from "../../lib/webAccess";
import { deriveWebUiView } from "./meshWebUi";
import { MeshWebUiControl, type WebUiAction } from "./MeshWebUiControl";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1/mesh-web-ui" });
  const g = globalThis as Record<string, unknown>;
  for (const key of ["window", "document", "HTMLElement", "HTMLButtonElement", "navigator"]) {
    Object.defineProperty(globalThis, key, {
      value: (window as unknown as Record<string, unknown>)[key],
      configurable: true,
    });
  }
  // happy-dom lacks a real clipboard; stub the one call the control makes.
  g.__copied = null;
  Object.defineProperty(globalThis.navigator, "clipboard", {
    value: { writeText: async (t: string) => { g.__copied = t; } },
    configurable: true,
  });
}

function snap(state: ServeReconcileState, over: Partial<ServeReconcileSnapshot> = {}): ServeReconcileSnapshot {
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

async function render(
  snapshot: ServeReconcileSnapshot | null,
  loading = false,
  onAction: (a: WebUiAction) => Promise<string | void> = async () => {},
  onOpenSurface: () => void = () => {},
): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <MeshWebUiControl
        snapshot={snapshot}
        loading={loading}
        onOpenSurface={onOpenSurface}
        onAction={onAction}
      />,
    );
  });
  return { host, root };
}

const q = (sel: string) => document.body.querySelector(sel) as HTMLElement | null;
async function openMenu(host: HTMLDivElement) {
  const trigger = host.querySelector('[data-testid="mesh-web-ui-menu-trigger"]') as HTMLButtonElement;
  assert(trigger, "menu trigger present");
  await act(async () => trigger.click());
}

// ── pure view derivation ────────────────────────────────────────────────────

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

// ── rendered control ────────────────────────────────────────────────────────

test("control: ready renders only the Web UI surface without redundant version or Open action", async () => {
  const { host, root } = await render(snap("ready"));
  const action = host.querySelector('[data-testid="mesh-web-ui-action"]') as HTMLElement;
  assert(action, "action root present");
  assert(action.getAttribute("data-web-ui-state") === "ready", "data-web-ui-state=ready");
  assert(!host.querySelector('[data-testid="mesh-web-ui-open"]'), "redundant Open button is absent");
  assert(!host.querySelector('[data-testid="mesh-web-ui-version"]'), "daemon version line is absent");
  await act(async () => root.unmount());
});

test("control: non-ready states preserve their state", async () => {
  for (const st of ["setup_required", "conflict", "unhealthy", "operator_bootstrap_required"] as ServeReconcileState[]) {
    const { host, root } = await render(snap(st));
    const action = host.querySelector('[data-testid="mesh-web-ui-action"]') as HTMLElement;
    assert(action.getAttribute("data-web-ui-state") === st, `state attr ${st}`);
    await act(async () => root.unmount());
  }
});

test("control: ready menu shows Copy URL + Disable, not Setup/Repair", async () => {
  const { host, root } = await render(snap("ready"));
  await openMenu(host);
  assert(q('[data-testid="mesh-web-ui-menu"]'), "menu open");
  assert(q('[data-testid="mesh-web-ui-copy-url"]'), "copy url item");
  assert(q('[data-testid="mesh-web-ui-disable"]'), "disable item");
  assert(!q('[data-testid="mesh-web-ui-setup"]'), "no setup item");
  assert(!q('[data-testid="mesh-web-ui-repair"]'), "no repair item");
  await act(async () => root.unmount());
});

test("control: conflict menu shows Repair, not Setup/Disable", async () => {
  const { host, root } = await render(snap("conflict"));
  await openMenu(host);
  assert(q('[data-testid="mesh-web-ui-repair"]'), "repair item present");
  assert(!q('[data-testid="mesh-web-ui-setup"]'), "no setup item");
  assert(!q('[data-testid="mesh-web-ui-disable"]'), "no disable when not owned");
  await act(async () => root.unmount());
});

test("control: setup action invokes handler and shows inline message (no toast)", async () => {
  let called: WebUiAction | null = null;
  const { host, root } = await render(snap("setup_required"), false, async (a) => {
    called = a;
    return "Web UI enabled.";
  });
  await openMenu(host);
  const setup = q('[data-testid="mesh-web-ui-setup"]') as HTMLButtonElement;
  await act(async () => setup.click());
  assert(called === "setup", "onAction called with setup");
  assert(host.querySelector('[data-testid="mesh-web-ui-message"]'), "inline message rendered");
  await act(async () => root.unmount());
});

test("control: action failure surfaces an inline error", async () => {
  const { host, root } = await render(snap("unhealthy"), false, async () => {
    throw new Error("serve denied");
  });
  await openMenu(host);
  const repair = q('[data-testid="mesh-web-ui-repair"]') as HTMLButtonElement;
  await act(async () => repair.click());
  const err = host.querySelector('[data-testid="mesh-web-ui-error"]');
  assert(err && err.textContent?.includes("serve denied"), "inline error shown");
  await act(async () => root.unmount());
});

test("control: Web UI surface button opens the QR/link surface in any state", async () => {
  let surfaced = 0;
  const { host, root } = await render(snap("setup_required"), false, async () => {}, () => { surfaced++; });
  const surface = host.querySelector('[data-testid="mesh-web-ui-surface"]') as HTMLButtonElement;
  assert(surface, "surface button present even when not ready");
  await act(async () => surface.click());
  assert(surfaced === 1, "surface open invoked");
  await act(async () => root.unmount());
});

test("control: loading shows Checking chip and no menu trigger", async () => {
  const { host, root } = await render(null, true);
  const action = host.querySelector('[data-testid="mesh-web-ui-action"]') as HTMLElement;
  assert(action.getAttribute("data-web-ui-state") === "loading", "loading state");
  assert(!host.querySelector('[data-testid="mesh-web-ui-menu-trigger"]'), "no menu while loading");
  await act(async () => root.unmount());
});

async function main() {
  setupDom();
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.stack : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} MeshWebUiControl tests passed`);
}

main();
