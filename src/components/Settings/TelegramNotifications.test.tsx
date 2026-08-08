// Run: npx tsx src/components/Settings/TelegramNotifications.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest } from "../../lib/bridge";
import { TelegramNotifications } from "./TelegramNotifications";
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
    "HTMLButtonElement",
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

async function testRecipientsInputRoundTripsParsedList() {
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

      const prototype = Object.getPrototypeOf(input) as HTMLElement;
      Object.getOwnPropertyDescriptor(prototype, "value")?.set?.call(input, " 123 , 456 ,, ");
      await act(async () => {
        input.dispatchEvent(new window.Event("input", { bubbles: true }));
        input.dispatchEvent(new window.Event("change", { bubbles: true }));
      });

      const last = updates.at(-1);
      assert(last?.section === "notifications", "recipients edit must patch the notifications section");
      const telegram = (last?.patch as { telegram?: { recipients?: string[] } }).telegram;
      assert(
        JSON.stringify(telegram?.recipients) === JSON.stringify(["123", "456"]),
        `recipients must be trimmed and empties dropped, got ${JSON.stringify(telegram?.recipients)}`,
      );
      await act(async () => root.unmount());
    },
  );
}

async function main() {
  await testDeliverableGateRendersControl();
  await testUndeliverableGateHidesControl();
  await testGateFailureFailsClosed();
  await testRecipientsInputRoundTripsParsedList();
  console.log("PASS TelegramNotifications deliverability gate");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
