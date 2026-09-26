import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  __setInvokeForTest,
  checkProviderIntegration,
  installProviderIntegration,
  uninstallProviderIntegration,
} from "../../lib/bridge";
import type { ProviderDescriptor } from "../../lib/types";
import {
  ProviderIntegrationHealthCard,
  providersWithIntegrationHealth,
} from "./ProviderIntegrationHealthCard";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "Element", "Node", "Event"]) {
    Object.defineProperty(globalThis, key, { value: window[key as keyof Window], configurable: true });
  }
}

async function renderCard(): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <ProviderIntegrationHealthCard
        provider={{ id: "provider-alpha", display_name: "Provider Alpha" }}
      />,
    );
  });
  return { host, root };
}

async function flush() {
  await act(async () => {});
}

async function cleanup(root: Root) {
  await act(async () => root.unmount());
  document.body.replaceChildren();
}

async function withInvoke(
  invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>,
  body: () => Promise<void>,
) {
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => (
    invoke(command, args) as Promise<T>
  ));
  try {
    await body();
  } finally {
    __setInvokeForTest(null);
  }
}

setupDom();

async function testCapabilitySelectsIntegrationProviders() {
  const provider = (id: string, hasHooks: boolean) => ({
    id,
    display_name: id,
    has_hooks: hasHooks,
  } as ProviderDescriptor);
  const selected = providersWithIntegrationHealth([
    provider("provider-with-hooks", true),
    provider("provider-without-hooks", false),
  ]);
  assert(selected.length === 1, "only providers declaring the hook capability get a health surface");
  assert(selected[0]?.id === "provider-with-hooks", "selection follows capability instead of provider identity");
}

async function testStaleHookIsActionable() {
  await withInvoke(async (command) => {
    assert(command === "check_provider_integration", "mount must read real provider integration health");
    return { hooks_installed: false, precompact_hook: true, session_start_hook: false };
  }, async () => {
    const { host, root } = await renderCard();
    await flush();
    const surface = host.querySelector('[data-provider-integration-state="needs-repair"]');
    const repair = host.querySelector('[data-provider-integration-action="repair"]') as HTMLButtonElement | null;
    assert(surface, "a stale session-start hook must expose the needs-repair state");
    assert(repair && !repair.disabled, "a stale session-start hook must expose an enabled repair control");
    await cleanup(root);
  });
}

async function testCodexIntegrationStatesStayDistinguishable() {
  for (const [reported, surfaced] of [
    ["absent", "absent"],
    ["stale_version", "stale-version"],
    ["untrusted_hash", "untrusted-hash"],
  ] as const) {
    await withInvoke(async () => ({ integration_state: reported }), async () => {
      const { host, root } = await renderCard();
      await flush();
      assert(
        host.querySelector(`[data-provider-integration-state="${surfaced}"]`),
        `integration state ${reported} reaches its distinct UI state constant`,
      );
      assert(
        host.querySelector('[data-provider-integration-action="repair"]'),
        `integration state ${reported} exposes its recovery action`,
      );
      await cleanup(root);
    });
  }
}

async function testHealthyHookHasNoRepairAction() {
  await withInvoke(async () => ({ hooks_installed: true, session_start_hook: true }), async () => {
    const { host, root } = await renderCard();
    await flush();
    assert(host.querySelector('[data-provider-integration-state="healthy"]'), "healthy check result must resolve the surfaced state");
    assert(!host.querySelector('[data-provider-integration-action="repair"]'), "healthy integration must not offer repair");
    const uninstall = host.querySelector('[data-provider-integration-action="uninstall"]') as HTMLButtonElement | null;
    assert(uninstall && !uninstall.disabled, "healthy integration must expose an enabled uninstall control");
    await cleanup(root);
  });
}

async function testFallbackBindingIsVisibleInIntegrationStatus() {
  await withInvoke(async () => ({
    integration_state: "installed",
    codeterm_binding_fallback_source: "launch_marker",
  }), async () => {
    const { host, root } = await renderCard();
    await flush();
    assert(host.querySelector('[data-provider-integration-state="healthy"]'), "installed hook health remains distinct from launch binding evidence");
    const fallback = host.querySelector('[data-provider-integration-binding-fallback="launch_marker"]');
    assert(fallback?.textContent?.trim(), "integration status surfaces the observed fallback binding source");
    await cleanup(root);
  });
}

async function testMissingStopHookIsDegraded() {
  await withInvoke(async () => (
    {
      hooks_installed: false,
      precompact_hook: true,
      stop_hook: false,
      session_start_hook: true,
      repair_requires_consent: true,
    }
  ), async () => {
    const { host, root } = await renderCard();
    await flush();
    assert(
      host.querySelector('[data-provider-integration-state="needs-repair"]'),
      "a missing stop hook must expose the needs-repair state even when session-start is healthy",
    );
    const alert = host.querySelector("[data-provider-integration-stale-hooks]");
    assert(
      alert?.getAttribute("data-provider-integration-stale-hooks") === "stop",
      "the degraded surface must name the stop hook as the stale field",
    );
    assert(
      alert?.getAttribute("data-provider-integration-action-required") === "explicit-consent",
      "checkIntegration's missing-stop state must reach the user-facing consent action",
    );
    const repair = host.querySelector('[data-provider-integration-action="repair"]') as HTMLButtonElement | null;
    assert(repair && !repair.disabled, "a missing stop hook must expose an enabled repair control");
    assert(
      repair.getAttribute("data-provider-integration-consent") === "explicit",
      "the only write path must remain the user's explicit repair action",
    );
    await cleanup(root);
  });
}

async function testStopHookHealthyWithSessionStart() {
  await withInvoke(async () => (
    { hooks_installed: true, stop_hook: true, session_start_hook: true }
  ), async () => {
    const { host, root } = await renderCard();
    await flush();
    assert(
      host.querySelector('[data-provider-integration-state="healthy"]'),
      "stop and session-start hooks both installed must resolve healthy",
    );
    assert(!host.querySelector("[data-provider-integration-stale-hooks]"), "healthy integration must not surface stale hooks");
    await cleanup(root);
  });
}

async function testAbsentStopHookFieldIsNotDegraded() {
  await withInvoke(async () => ({ hooks_installed: true, session_start_hook: true }), async () => {
    const { host, root } = await renderCard();
    await flush();
    assert(
      host.querySelector('[data-provider-integration-state="healthy"]'),
      "a provider that does not report stop_hook must not be marked degraded for it",
    );
    assert(!host.querySelector("[data-provider-integration-stale-hooks]"), "an absent stop_hook field must render no stale-hook surface");
    await cleanup(root);
  });
}

async function testRepairRefetchesIntoHealthyState() {
  const commands: string[] = [];
  let checks = 0;
  let installArgs: Record<string, unknown> | undefined;
  await withInvoke(async (command, args) => {
    commands.push(command);
    if (command === "check_provider_integration") {
      checks += 1;
      return checks === 1
        ? { hooks_installed: false, precompact_hook: true, session_start_hook: false }
        : { hooks_installed: true, session_start_hook: true };
    }
    if (command === "install_provider_integration") {
      installArgs = args;
      return undefined;
    }
    throw new Error(`unexpected command: ${command}`);
  }, async () => {
    const { host, root } = await renderCard();
    await flush();
    const repair = host.querySelector('[data-provider-integration-action="repair"]') as HTMLButtonElement | null;
    assert(repair, "repair control must exist before exercising the repair flow");
    await act(async () => repair.click());
    await flush();
    assert(
      commands.join(",") === "check_provider_integration,install_provider_integration,check_provider_integration",
      "repair must invoke install once and then re-check once",
    );
    assert(installArgs?.consent === true, "repair must send explicit user consent to the existing installer");
    assert(installArgs?.precompactMemoryExport === undefined, "repair delegates PreCompact preservation to the daemon");
    assert(host.querySelector('[data-provider-integration-state="healthy"]'), "refetched health must update the UI without remounting");
    assert(!host.querySelector('[data-provider-integration-action="repair"]'), "reactive healthy state must remove the repair control");
    await cleanup(root);
  });
}

async function testBridgeWrappersUseRegisteredCommands() {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  await withInvoke(async (command, args) => {
    calls.push({ command, args });
    return command === "check_provider_integration" ? { session_start_hook: true } : undefined;
  }, async () => {
    await checkProviderIntegration("provider-beta");
    await installProviderIntegration("provider-beta", true);
    await installProviderIntegration("provider-beta");
    await uninstallProviderIntegration("provider-beta");
  });
  assert(
    calls.map((call) => call.command).join(",") ===
      "check_provider_integration,install_provider_integration,install_provider_integration,uninstall_provider_integration",
    "bridge wrappers must call the registered provider integration commands",
  );
  assert(calls.every((call) => call.args?.providerId === "provider-beta"), "wrappers must forward the selected provider id");
  assert(calls[1]?.args?.consent === true, "the user-triggered install wrapper must record explicit consent");
  assert(calls[1]?.args?.precompactMemoryExport === true, "repair must preserve the checked pre-compact hook state");
  assert(calls[2]?.args?.precompactMemoryExport === undefined, "an omitted choice stays omitted through the bridge");
}

async function main() {
  await testCapabilitySelectsIntegrationProviders();
  await testStaleHookIsActionable();
  await testCodexIntegrationStatesStayDistinguishable();
  await testHealthyHookHasNoRepairAction();
  await testFallbackBindingIsVisibleInIntegrationStatus();
  await testMissingStopHookIsDegraded();
  await testStopHookHealthyWithSessionStart();
  await testAbsentStopHookFieldIsNotDegraded();
  await testRepairRefetchesIntoHealthyState();
  await testBridgeWrappersUseRegisteredCommands();
  console.log("PASS provider integration health and reactive repair");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
