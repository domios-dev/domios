import { describe, expect, it } from "vitest";
import type { HelperMemory } from "./helperRules";
import { createHelperMomentsStore, type HelperMomentsPorts } from "./helperMomentsStore";
import { MOMENT_THRESHOLDS, type HelperTabKind } from "./momentRegistry";

function setup(overrides: Partial<HelperMomentsPorts> = {}) {
  const saved: HelperMemory[] = [];
  const kinds: Record<string, HelperTabKind> = { agent: "agent", shell: "manual-shell" };
  const store = createHelperMomentsStore({
    load: () => null,
    save: (memory) => saved.push(memory),
    tabKind: (tabId) => kinds[tabId] ?? "other",
    blueprintsInUse: async () => false,
    ...overrides,
  });
  store.setFacts({ telegramConnected: false, plugins: [{ id: "jira-client", name: "Jira", installed: false }] });
  return { store, saved };
}

async function waitAway(store: ReturnType<typeof setup>["store"]) {
  await store.observe({ kind: "window-focus", focused: false, at: 0 });
  await store.observe({ kind: "tab-status", tabId: "agent", status: "Waiting", at: 0 });
  return store.observe({ kind: "window-focus", focused: true, at: MOMENT_THRESHOLDS.awayMs });
}

describe("helper moments store", () => {
  it("holds one suggestion and drops competing moments", async () => {
    const { store } = setup();
    expect(await waitAway(store)).toEqual({ admit: true });
    expect(await store.observe({ kind: "user-message", text: "Create a Jira ticket", at: 1 })).toEqual({ admit: false, reason: "busy" });
    expect(store.getSnapshot().active?.momentId).toBe("waiting_on_user_while_away");
  });

  it("persists showing once, then dismissal, and never repeats the suggestion", async () => {
    const { store, saved } = setup();
    await waitAway(store);
    store.markShown();
    store.markShown();
    store.dismiss();
    expect(store.getSnapshot().active).toBeNull();
    expect(saved[saved.length - 1]?.moments.waiting_on_user_while_away).toMatchObject({ shown: 1, dismissed: 1, muted: false });
    expect(await waitAway(store)).toEqual({ admit: false, reason: "seen" });
  });

  it("restores Don't show again from storage", async () => {
    const first = setup();
    await waitAway(first.store);
    first.store.markShown();
    first.store.mute();
    const { store } = setup({ load: () => JSON.parse(JSON.stringify(first.saved[first.saved.length - 1])) });
    expect(await waitAway(store)).toEqual({ admit: false, reason: "muted" });
  });

  it("counts acceptance once and closes without a dismissal", async () => {
    const { store, saved } = setup();
    await waitAway(store);
    store.markShown();
    store.accept();
    store.accept();
    expect(store.getSnapshot().active?.accepted).toBe(true);
    store.dismiss();
    expect(saved[saved.length - 1]?.moments.waiting_on_user_while_away).toMatchObject({ shown: 1, accepted: 1, dismissed: 0 });
  });

  it("withdraws an unaccepted suggestion once the feature gets connected elsewhere", async () => {
    const { store } = setup();
    await waitAway(store);
    store.setFacts({ telegramConnected: true });
    expect(store.getSnapshot().active).toBeNull();
  });

  it("asks the workspace about Blueprints before suggesting one", async () => {
    const commands = ["make", "./deploy.sh", "make", "./deploy.sh", "make", "./deploy.sh"];
    for (const [inUse, expected] of [[true, { admit: false, reason: "in-use" }], [false, { admit: true }]] as const) {
      const { store } = setup({ blueprintsInUse: async () => inUse });
      let verdict = null;
      for (const commandLine of commands) verdict = await store.observe({ kind: "shell-command", tabId: "shell", commandLine, at: 0 });
      expect(verdict).toEqual(expected);
    }
  });

  it("offers an adjacent block only for the answer to the user's latest message", async () => {
    const { store } = setup();
    expect(store.offerAdjacent("@ds-plugin jira-client")).toBeNull();
    store.setFacts({ plugins: [{ id: "telegram-client", name: "Telegram Client", installed: false }] });
    await store.observe({ kind: "user-message", text: "what can you do with plugins?", at: 0 });
    expect(store.offerAdjacent("@ds-plugin telegram-client")).toEqual({ admit: true });
    expect(store.getSnapshot().active).toMatchObject({ momentId: "adjacent_capability", block: { kind: "guide", sectionId: "phone" } });
    store.discard();
    expect(store.offerAdjacent("@ds-plugin telegram-client")).toBeNull();
  });
});
