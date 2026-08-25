// Run: npx tsx src/components/Settings/TelegramNotifications.test.tsx
//
// Telegram's own channel editor. Whether it is offered at all is the registry's
// answer and is covered by the channel_registry suite; this file covers what
// the editor itself does once mounted.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { TelegramNotifications, TELEGRAM_CHANNEL_ID } from "./TelegramNotifications";
import { channelPatch, parseRecipients } from "../../lib/pushChannels";
import type { AppConfig, PushChannelConfig } from "../../lib/types";

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

function configWith(audience?: PushChannelConfig): AppConfig {
  return {
    notifications: audience ? { channels: { [TELEGRAM_CHANNEL_ID]: audience } } : {},
  } as unknown as AppConfig;
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

setupDom();

const CONTROL = '[data-testid="settings-telegram-notifications"]';

async function testMountedEditorOffersTheOptIn() {
  const updates: UpdateCall[] = [];
  const { host, root } = await render(configWith(), updates);
  assert(host.querySelector(CONTROL), "a mounted channel editor renders its opt-in control");
  const toggle = host.querySelector('button[role="switch"]') as HTMLButtonElement | null;
  assert(toggle, "the opt-in is an accessible switch");
  assert(toggle.getAttribute("aria-checked") === "false", "an absent entry reads as opted out");
  await act(async () => toggle.click());
  assert(updates.length === 1, `one patch per flip, got ${updates.length}`);
  assert(updates[0].section === "notifications", "the patch targets the notifications section");
  const channels = updates[0].patch.channels as Record<string, PushChannelConfig>;
  assert(
    channels[TELEGRAM_CHANNEL_ID].enabled,
    "the flip writes this channel's own keyed entry",
  );
  await act(async () => root.unmount());
}

async function testEnabledOptInRendersAccessibleRecipientsInput() {
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
}

function testRecipientsParsingAndPatchContract() {
  assert(
    JSON.stringify(parseRecipients(" 123 , 456 ,, ")) === JSON.stringify(["123", "456"]),
    `recipients must be trimmed and empties dropped, got ${JSON.stringify(parseRecipients(" 123 , 456 ,, "))}`,
  );
  assert(JSON.stringify(parseRecipients("")) === "[]", "empty text must parse to no recipients");

  const patch = channelPatch(
    { [TELEGRAM_CHANNEL_ID]: { enabled: true, recipients: [], per_scope_limit: 12 } },
    TELEGRAM_CHANNEL_ID,
    { recipients: parseRecipients(" 123 , 456 ,, ") },
  );
  assert(
    JSON.stringify(patch) ===
      JSON.stringify({
        channels: { telegram: { enabled: true, recipients: ["123", "456"], per_scope_limit: 12 } },
      }),
    `recipients patch must keep current settings and carry the parsed list, got ${JSON.stringify(patch)}`,
  );

  const defaulted = channelPatch(undefined, TELEGRAM_CHANNEL_ID, { enabled: true });
  assert(
    JSON.stringify(defaulted) ===
      JSON.stringify({
        channels: { telegram: { enabled: true, recipients: [], per_scope_limit: 12 } },
      }),
    `patch from an absent config must apply the documented defaults, got ${JSON.stringify(defaulted)}`,
  );

  // One channel's entry never disturbs another's.
  const alongside = channelPatch(
    { other: { enabled: true, recipients: ["9"], per_scope_limit: 3 } },
    TELEGRAM_CHANNEL_ID,
    { enabled: true },
  );
  assert(
    alongside.channels.other.recipients[0] === "9",
    "patching one channel keeps every other channel's entry intact",
  );
}

async function main() {
  await testMountedEditorOffersTheOptIn();
  await testEnabledOptInRendersAccessibleRecipientsInput();
  testRecipientsParsingAndPatchContract();
  console.log("PASS TelegramNotifications channel editor");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
