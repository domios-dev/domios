// Run: npx tsx src/components/ProviderUpdatePromptLayer.test.tsx

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../lib/bridge";
import {
  ProviderUpdatePromptLayer,
  type ProviderUpdatePrompt,
} from "./ProviderUpdatePromptLayer";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
}

function prompt(
  status: ProviderUpdatePrompt["status"],
  message: string,
): ProviderUpdatePrompt {
  return {
    prompt_id: "prompt-1",
    tab_id: "tab-1",
    provider_name: "Fixture Provider",
    restart_required: true,
    status,
    message,
  };
}

async function main() {
  setupDom();
  const listeners = new Map<string, (event: { payload: ProviderUpdatePrompt }) => void>();
  const responses: Array<{ promptId: string; accept: boolean }> = [];
  const acknowledgements: string[] = [];
  let releaseAcknowledgement: (() => void) | undefined;
  let backendPrompts = [prompt("awaiting_consent", "Permission is required.")];
  __setInvokeForTest(async <T,>(command: string, args): Promise<T> => {
    if (command === "list_provider_update_prompts") {
      return backendPrompts as T;
    }
    if (command === "acknowledge_provider_update_prompt") {
      const { promptId } = args as { promptId: string };
      acknowledgements.push(promptId);
      const index = backendPrompts.findIndex((item) => item.prompt_id === promptId);
      assert(index >= 0, "acknowledgement must identify a listed prompt");
      await new Promise<void>((resolve) => {
        releaseAcknowledgement = resolve;
      });
      backendPrompts = backendPrompts.filter((item) => item.prompt_id !== promptId);
      return undefined as T;
    }
    assert(command === "respond_provider_update_prompt", "unexpected provider update command");
    responses.push(args as { promptId: string; accept: boolean });
    return undefined as T;
  });
  __setListenForTest(async (event, handler) => {
    listeners.set(event, handler as (event: { payload: ProviderUpdatePrompt }) => void);
    return () => listeners.delete(event);
  });

  const host = document.createElement("div");
  document.body.appendChild(host);
  let root = createRoot(host);
  await act(async () => {
    root.render(<ProviderUpdatePromptLayer />);
    await Promise.resolve();
  });

  const dialog = host.querySelector('[role="dialog"][aria-modal="true"]');
  assert(dialog, "provider update prompt must be an accessible modal dialog");
  assert(dialog.getAttribute("aria-labelledby") === "provider-update-title", "dialog must name its heading");
  const buttons = Array.from(dialog.querySelectorAll("button"));
  assert(buttons.some((button) => button.textContent === "Decline"), "awaiting consent exposes Decline");
  const install = buttons.find((button) => button.textContent === "Install update");
  assert(install, "awaiting consent exposes Install update");
  await act(async () => {
    (install as HTMLButtonElement).click();
    await Promise.resolve();
  });
  assert(responses.length === 1 && responses[0].accept, "install records one explicit acceptance");

  await act(async () => {
    listeners.get("provider-update-prompt")?.({
      payload: prompt("deferred", "PendingDeliveryRecord is Unconfirmed."),
    });
  });
  assert(host.textContent?.includes("PendingDeliveryRecord is Unconfirmed"), "deferred reason remains visible");
  assert(!host.textContent?.includes("Install update"), "consumed authority cannot be replayed while deferred");
  assert(!host.textContent?.includes("Dismiss"), "deferred state cannot be silently dismissed");

  backendPrompts = [prompt("restarted", "Completion observed and readiness reached.")];
  await act(async () => {
    listeners.get("provider-update-prompt")?.({
      payload: backendPrompts[0],
    });
  });
  assert(host.textContent?.includes("Completion observed and readiness reached"), "terminal status is visible");
  assert(host.textContent?.includes("Dismiss"), "terminal status exposes Dismiss");
  assert(!host.textContent?.includes("Decline"), "terminal status exposes no consent action");

  await act(async () => root.unmount());
  root = createRoot(host);
  await act(async () => {
    root.render(<ProviderUpdatePromptLayer />);
    await Promise.resolve();
  });
  assert(host.textContent?.includes("Completion observed and readiness reached"), "unacknowledged terminal status survives remount");
  const dismiss = Array.from(host.querySelectorAll("button")).find(
    (button) => button.textContent === "Dismiss",
  );
  assert(dismiss, "remounted terminal status exposes Dismiss");
  await act(async () => {
    (dismiss as HTMLButtonElement).click();
    await Promise.resolve();
  });
  assert(releaseAcknowledgement, "Dismiss waits for backend acknowledgement");
  assert(host.querySelector('[role="dialog"]'), "prompt remains visible before acknowledgement succeeds");
  await act(async () => {
    releaseAcknowledgement?.();
    await Promise.resolve();
  });
  assert(acknowledgements.length === 1 && acknowledgements[0] === "prompt-1", "Dismiss acknowledges the exact terminal prompt");
  assert(!host.querySelector('[role="dialog"]'), "prompt is removed locally only after acknowledgement succeeds");

  await act(async () => root.unmount());
  root = createRoot(host);
  await act(async () => {
    root.render(<ProviderUpdatePromptLayer />);
    await Promise.resolve();
  });
  assert(!host.querySelector('[role="dialog"]'), "acknowledged terminal status stays absent after remount");
  await act(async () => root.unmount());
  __setInvokeForTest(null);
  __setListenForTest(null);
  console.log("\n✓ ProviderUpdatePromptLayer accessible state test passed");
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
