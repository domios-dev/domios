import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { GuideNavigationProvider } from "@codeterm/chat-ui";
import { __setInvokeForTest } from "../../lib/bridge";
import { createHelperMomentsStore, type HelperMomentsStore } from "../../lib/helperMoments/helperMomentsStore";
import { MOMENT_THRESHOLDS } from "../../lib/helperMoments/momentRegistry";
import { HelperMomentCard } from "./HelperMomentCard";

Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });

let host: HTMLDivElement;
let root: Root;
const opened: string[] = [];

async function storeWithPhoneSuggestion(): Promise<HelperMomentsStore> {
  const store = createHelperMomentsStore({ load: () => null, save: () => {}, tabKind: () => "agent", blueprintsInUse: async () => null });
  store.setFacts({ telegramConnected: false });
  await store.observe({ kind: "window-focus", focused: false, at: 0 });
  await store.observe({ kind: "tab-status", tabId: "agent", status: "Waiting", at: 0 });
  await store.observe({ kind: "window-focus", focused: true, at: MOMENT_THRESHOLDS.awayMs });
  return store;
}

async function render(store: HelperMomentsStore, props: { idle?: boolean; latestAnswer?: string | null } = {}) {
  await act(async () => root.render(
    <GuideNavigationProvider sections={[{ id: "phone", title: "Answer your agents from your phone" }]} onOpen={(id) => opened.push(id)}>
      <HelperMomentCard store={store} idle={props.idle ?? true} latestAnswer={props.latestAnswer ?? null} />
    </GuideNavigationProvider>,
  ));
}

function button(name: string): HTMLButtonElement {
  const found = [...host.querySelectorAll("button")].find((candidate) => (candidate.getAttribute("aria-label") ?? candidate.textContent) === name);
  if (!found) throw new Error(`no button ${name}`);
  return found;
}

beforeEach(() => {
  __setInvokeForTest(async () => {
    throw new Error("offline");
  });
  opened.length = 0;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  __setInvokeForTest(null);
});

describe("HelperMomentCard", () => {
  it("shows the suggestion with its live block while the agent is idle and counts one showing", async () => {
    const store = await storeWithPhoneSuggestion();
    await render(store, { idle: false });
    expect(host.querySelector('[aria-label="Suggestion from Domios"]')).toBeNull();
    await render(store);
    expect(host.querySelector('[aria-label="Suggestion from Domios"] [data-chat-block="guide"]')).not.toBeNull();
    await render(store);
    expect(store.getSnapshot().memory.moments.waiting_on_user_while_away?.shown).toBe(1);
  });

  it("counts a block action as acceptance, then Close hides it without a dismissal", async () => {
    const store = await storeWithPhoneSuggestion();
    await render(store);
    await act(async () => button("Open Guide: Answer your agents from your phone").click());
    expect(opened).toEqual(["phone"]);
    expect(store.getSnapshot().memory.moments.waiting_on_user_while_away).toMatchObject({ accepted: 1, dismissed: 0 });
    await act(async () => button("Close").click());
    expect(host.querySelector('[aria-label="Suggestion from Domios"]')).toBeNull();
  });

  it("Not now dismisses once and Don't show again mutes the moment", async () => {
    const dismissed = await storeWithPhoneSuggestion();
    await render(dismissed);
    await act(async () => button("Not now").click());
    expect(dismissed.getSnapshot().memory.moments.waiting_on_user_while_away).toMatchObject({ dismissed: 1, muted: false });
    expect(host.querySelector('[aria-label="Suggestion from Domios"]')).toBeNull();

    const muted = await storeWithPhoneSuggestion();
    await render(muted);
    await act(async () => button("Don't show again").click());
    expect(muted.getSnapshot().memory.moments.waiting_on_user_while_away).toMatchObject({ dismissed: 1, muted: true });
  });

  it("stands down when the agent's own answer already shows the same block", async () => {
    const store = await storeWithPhoneSuggestion();
    await render(store, { latestAnswer: "Connect your phone:\n\n@ds-guide phone" });
    expect(host.querySelector('[aria-label="Suggestion from Domios"]')).toBeNull();
    expect(store.getSnapshot().active).toBeNull();
    expect(store.getSnapshot().memory.moments.waiting_on_user_while_away?.shown ?? 0).toBe(0);
  });
});
