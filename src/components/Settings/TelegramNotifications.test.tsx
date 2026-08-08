// Run: npx tsx src/components/Settings/TelegramNotifications.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest } from "../../lib/bridge";
import { TelegramNotifications, parseRecipients, telegramPatch } from "./TelegramNotifications";
import type { AppConfig } from "../../lib/types";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLInputElement",
    "HTMLTextAreaElement",
    "HTMLSelectElement",
    "HTMLButtonElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
  ]) {
    Object.defineProperty(globalThis, key, { value: window[key as keyof Window], configurable: true });
  }
}


function configWith(telegram?: { enabled: boolean; recipients: string[]; per_scope_limit: number }): AppConfig {
  return { notifications: telegram ? { telegram } : {} } as unknown as AppConfig;
}

type UpdateCall = { section: string; patch: Record<string, unknown> };

async function render(
  config: AppConfig,
  updates: UpdateCall[],
): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <TelegramNotifications
        config={config}
        onUpdate={(section, patch) => updates.push({ section, patch: patch as Record<string, unknown> })}
      />,
    ),
  );
  return { host, root };
}

async function withInvoke(
  invoke: (command: string) => Promise<unknown>,
  body: () => Promise<void>,
) {
  __setInvokeForTest(async <T,>(command: string) => invoke(command) as Promise<T>);
  try {
    await body();
  } finally {
    __setInvokeForTest(null);
  }
}

setupDom();

const CONTROL = '[data-testid="settings-telegram-notifications"]';

async function testDeliverableGateRendersControl() {
  await withInvoke(
    async (command) => {
      if (command === "telegram_push_gate") return { control_available: true };
      throw new Error(`unexpected command: ${command}`);
    },
    async () => {
      const updates: UpdateCall[] = [];
      const { host, root } = await render(configWith(), updates);
      assert(host.querySelector(CONTROL), "deliverable gate must render the opt-in control");
      await act(async () => root.unmount());
    },
  );
}

async function testUndeliverableGateHidesControl() {
  await withInvoke(
    async (command) => {
      if (command === "telegram_push_gate") return { control_available: false };
      throw new Error(`unexpected command: ${command}`);
    },
    async () => {
      const updates: UpdateCall[] = [];
      const { host, root } = await render(
        configWith({ enabled: true, recipients: ["123"], per_scope_limit: 12 }),
        updates,
      );
      assert(!host.querySelector(CONTROL), "undeliverable gate must render nothing, even when config opts in");
      await act(async () => root.unmount());
    },
  );
}

async function testGateFailureFailsClosed() {
  await withInvoke(
    async () => {
      throw new Error("daemon unreachable");
    },
    async () => {
      const updates: UpdateCall[] = [];
      const { host, root } = await render(configWith(), updates);
      assert(!host.querySelector(CONTROL), "a failed gate probe must hide the control, not default it open");
      await act(async () => root.unmount());
    },
  );
}

async function testEnabledOptInRendersAccessibleRecipientsInput() {
  await withInvoke(
    async (command) => {
      if (command === "telegram_push_gate") return { control_available: true };
      throw new Error(`unexpected command: ${command}`);
    },
    async () => {
      const updates: UpdateCall[] = [];
      const { host, root } = await render(
        configWith({ enabled: true, recipients: [], per_scope_limit: 12 }),
        updates,
      );
      const input = host.querySelector(
        '[data-testid="settings-telegram-push-recipients"]',
      ) as HTMLInputElement | null;
      assert(input, "enabled opt-in must expose the recipients input");
      assert(
        host.querySelector('label[for="telegram-push-recipients"]'),
        "recipients input must carry an associated label for accessibility",
      );
      await act(async () => root.unmount());
    },
  );
}

function testRecipientsParsingAndPatchContract() {
  assert(
    JSON.stringify(parseRecipients(" 123 , 456 ,, ")) === JSON.stringify(["123", "456"]),
    `recipients must be trimmed and empties dropped, got ${JSON.stringify(parseRecipients(" 123 , 456 ,, "))}`,
  );
  assert(JSON.stringify(parseRecipients("")) === "[]", "empty text must parse to no recipients");

  const patch = telegramPatch(
    { enabled: true, recipients: [], per_scope_limit: 12 },
    { recipients: parseRecipients(" 123 , 456 ,, ") },
  );
  assert(
    JSON.stringify(patch) ===
      JSON.stringify({ telegram: { enabled: true, recipients: ["123", "456"], per_scope_limit: 12 } }),
    `recipients patch must keep current settings and carry the parsed list, got ${JSON.stringify(patch)}`,
  );

  const defaulted = telegramPatch(undefined, { enabled: true });
  assert(
    JSON.stringify(defaulted) ===
      JSON.stringify({ telegram: { enabled: true, recipients: [], per_scope_limit: 12 } }),
    `patch from an absent config must apply the documented defaults, got ${JSON.stringify(defaulted)}`,
  );
}

async function main() {
  await testDeliverableGateRendersControl();
  await testUndeliverableGateHidesControl();
  await testGateFailureFailsClosed();
  await testEnabledOptInRendersAccessibleRecipientsInput();
  testRecipientsParsingAndPatchContract();
  console.log("PASS TelegramNotifications deliverability gate");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
