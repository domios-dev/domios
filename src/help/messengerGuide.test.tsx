// Run: npx tsx src/help/messengerGuide.test.tsx

import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { ChatBlockHostProvider, type ConnectBlockState, type PluginBlockState } from "@codeterm/chat-ui";
import { HelpPanel } from "./HelpPanel";
import type { AppConfig } from "../lib/types";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLSelectElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? testWindow : testWindow[key], configurable: true });
}

function ready(id: string, name: string): PluginBlockState {
  return {
    kind: "ready",
    plugin: { id, name, pitch: "", installed: false, updateAvailable: false, compatible: true, managed: false, hasView: true, pendingReload: false },
  };
}

(async () => {
  const states: Record<string, PluginBlockState> = { "telegram-client": ready("telegram-client", "Telegram Client"), "teams-client": ready("teams-client", "Teams Client") };
  const runs: string[] = [];
  const plugins = {
    binding: { subscribe: () => () => {}, read: (id: string) => states[id], refresh: () => {} },
    run: async (id: string) => { runs.push(id); },
    openView: () => {},
    openInPlugins: () => {},
  };
  const opened: string[] = [];
  const connects: Array<string | undefined> = [];
  const telegram: ConnectBlockState = {
    kind: "ready",
    bridge: {
      id: "telegram", name: "Telegram", description: "Message your General Agent from Telegram.", phase: "disconnected", tokenStored: true,
      method: { kind: "token", label: "Bot token", placeholder: "123:AA", help: "Paste the token." },
    },
  };
  const connect = {
    binding: { subscribe: () => () => {}, read: () => telegram, refresh: () => {} },
    connect: async (_id: string, token?: string) => { connects.push(token); },
    disconnect: async () => {},
    openSettings: () => {},
  };

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  let failed = 0;
  try {
    await act(async () => root.render(
      <ChatBlockHostProvider value={{ plugins, connect }}>
        <HelpPanel config={{ keybindings: {} } as AppConfig} onOpenSettings={(target) => opened.push(target)} />
      </ChatBlockHostProvider>,
    ));
    const messengers = host.querySelector('[data-help-section-id="messenger-plugins"]');
    assert(messengers, "messenger plugins guide section missing");
    const cards = [...messengers.querySelectorAll('[data-chat-block="plugin"]')];
    assert(cards.length === 2, `guide should embed two plugin cards, got ${cards.length}`);
    assert(cards.every((card) => card.querySelector('[data-block-action="install"]')), "embedded cards lost Install");
    assert(runs.length === 0, "rendering the guide installed something");
    await act(async () => (cards[0].querySelector('[data-block-action="install"]') as HTMLButtonElement).click());
    assert(runs.join(",") === "telegram-client", "embedded card install did not reach the host once");

    const phone = host.querySelector('[data-help-section-id="phone"]');
    const phoneBlocks = phone?.querySelectorAll('[data-chat-block]');
    const last = phoneBlocks?.[phoneBlocks.length - 1];
    assert(last?.getAttribute("data-chat-block") === "connect", "phone guide must end with the Telegram connect block");
    assert(connects.length === 0, "rendering the guide connected Telegram");
    await act(async () => (last.querySelector('[data-block-action="connect"]') as HTMLButtonElement).click());
    assert(JSON.stringify(connects) === "[null]", "the guide's connect block did not reach the host once with the saved token");

    assert(!host.querySelector('[data-help-section-id="integrations"]'), "the retired integrations section is still listed");
    const settings = host.querySelector('[data-help-section-id="messengers"] [data-help-settings-target="telegram-bridge"]') as HTMLButtonElement | null;
    assert(settings, "messengers guide lacks the Messengers settings link");
    await act(async () => settings.click());
    assert(opened.join(",") === "telegram-bridge", "messengers guide did not deep-link to General > Messengers");
    console.log("  ✓ guide_embeds_plugin_cards_and_connects_telegram");
  } catch (error) {
    failed++;
    console.error(`  ✗ guide_embeds_plugin_cards_and_connects_telegram: ${error instanceof Error ? error.message : error}`);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
  console.log(`help/messengerGuide tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
