/**
 * WHAT: the shared picker pin proves the stub per provider, then selects and
 *      asserts it on every head before a confirm can run.
 * WHY: a picker confirmed on an unstubbed provider launches the real LLM.
 * ORACLE: the pinned head state, the thrown provider name, and zero clicks on refusal.
 */
import assert from "node:assert/strict";
import test from "node:test";
import type { ApiClient } from "./api-client";
import {
  pinPickerToStub,
  requireStubbedProvider,
  type PinLocator,
  type PinPage,
  type StubPinDeps,
} from "./stub-pin";

const STUB_CMD = "node tests/playwright/fixtures/stub-agent/replay.mjs";
const CLAUDE = { id: "claude", display_name: "Claude Code", transport: "pty", stub_replay: true };

function fakeApi(providers: unknown[]): { api: Pick<ApiClient, "get">; posts: string[] } {
  const posts: string[] = [];
  const api = {
    async get<T>(p: string): Promise<T> {
      if (p === "/agents/providers") return providers as T;
      throw new Error(`unexpected GET ${p}`);
    },
    async post<T>(p: string): Promise<T> {
      posts.push(p);
      throw new Error(`pin must not POST ${p}`);
    },
  };
  return { api, posts };
}

function deps(env: NodeJS.ProcessEnv, transportActive = true): StubPinDeps {
  return { env, transportActive: async () => transportActive };
}

type Head = { value: string; selectedCard: string | null };

/** A picker whose menus offer `offered` (display name -> provider id). */
function fakePicker(offered: Record<string, string>) {
  const clicks: string[] = [];
  let open: Head | null = null;
  const matches = (head: Head, selector: string, card?: string): boolean => {
    if (selector === ':not([data-value=""])') return head.value !== "";
    if (selector === '[data-selected="yes"]') return card !== undefined && head.selectedCard === card;
    const value = selector.match(/^\[data-value="(.*)"\]$/)?.[1];
    if (value !== undefined) return head.value === value;
    throw new Error(`unexpected selector ${selector}`);
  };
  const probe = (selector: string): PinLocator => ({
    async click() {
      throw new Error(`probe ${selector} is not clickable`);
    },
    and() {
      throw new Error("unexpected and");
    },
    locator() {
      throw new Error("unexpected locator");
    },
    async waitFor() {
      throw new Error(`probe ${selector} is not a target`);
    },
    toString: () => selector,
  } as PinLocator);
  const conditioned = (head: Head, other: PinLocator, card?: string): PinLocator => ({
    async click() {},
    and() {
      throw new Error("unexpected and");
    },
    locator() {
      throw new Error("unexpected locator");
    },
    async waitFor() {
      if (!matches(head, String(other), card)) throw new Error(`timeout waiting for ${String(other)}`);
    },
  });
  const menuHead = (name: string, value = "codex"): { head: Head; trigger: PinLocator } => {
    const head: Head = { value, selectedCard: null };
    const trigger: PinLocator = {
      async click() {
        clicks.push(`open:${name}`);
        open = head;
      },
      and: (other) => conditioned(head, other),
      locator() {
        throw new Error("unexpected locator");
      },
      async waitFor() {},
    };
    return { head, trigger };
  };
  const cardsHead = (): { head: Head; scope: PinLocator } => {
    const head: Head = { value: "", selectedCard: "codex" };
    const scope: PinLocator = {
      async click() {},
      and() {
        throw new Error("unexpected and");
      },
      locator(selector: string) {
        const id = selector.match(/agent-picker-card-([^"]+)"/)?.[1] ?? "";
        return {
          async click() {
            clicks.push(`card:${id}`);
            head.selectedCard = id;
          },
          and: (other) => conditioned(head, other, id),
          locator() {
            throw new Error("unexpected locator");
          },
          async waitFor() {},
        };
      },
      async waitFor() {},
    };
    return { head, scope };
  };
  const page: PinPage = {
    locator: (selector) => probe(selector),
    getByRole: (_role, { name }) => ({
      async click() {
        const id = offered[name];
        if (!open || id === undefined) throw new Error(`no enabled menuitem ${name}`);
        clicks.push(`pick:${id}`);
        open.value = id;
        open = null;
      },
      and() {
        throw new Error("unexpected and");
      },
      locator() {
        throw new Error("unexpected locator");
      },
      async waitFor() {},
    }),
  };
  return { page, clicks, menuHead, cardsHead };
}

test("a fully stubbed provider pins every menu and card head", async () => {
  const { api, posts } = fakeApi([CLAUDE, { id: "codex", transport: "pty", stub_replay: true }]);
  const picker = fakePicker({ "Claude Code": "claude", Codex: "codex" });
  const orchestrator = picker.menuHead("orchestrator");
  const planner = picker.menuHead("planner");
  const cards = picker.cardsHead();
  const pinned = await pinPickerToStub(
    api,
    picker.page,
    [
      { kind: "menu", trigger: orchestrator.trigger },
      { kind: "menu", trigger: planner.trigger },
      { kind: "cards", scope: cards.scope },
    ],
    "claude",
    deps({ CODETERM_STUB_AGENT_CMD: STUB_CMD }),
  );
  assert.equal(pinned.id, "claude");
  assert.equal(orchestrator.head.value, "claude");
  assert.equal(planner.head.value, "claude");
  assert.equal(cards.head.selectedCard, "claude");
  assert.deepEqual(picker.clicks, ["open:orchestrator", "pick:claude", "open:planner", "pick:claude", "card:claude"]);
  assert.deepEqual(posts, []);
});

test("a provider absent from the stub list throws naming it, before any head is touched", async () => {
  const cases: { listing: unknown[]; env: NodeJS.ProcessEnv; provider: string }[] = [
    { listing: [CLAUDE], env: { CODETERM_STUB_AGENT_CMD: STUB_CMD, CODETERM_STUB_AGENT_PROVIDERS: "codex" }, provider: "claude" },
    { listing: [{ ...CLAUDE, stub_replay: false }], env: { CODETERM_STUB_AGENT_CMD: STUB_CMD }, provider: "claude" },
    { listing: [CLAUDE], env: { CODETERM_STUB_AGENT_CMD: STUB_CMD }, provider: "opencode" },
    { listing: [{ ...CLAUDE, agent_runtime: {} }], env: { CODETERM_STUB_AGENT_CMD: STUB_CMD }, provider: "claude" },
  ];
  for (const { listing, env, provider } of cases) {
    const { api, posts } = fakeApi(listing);
    const picker = fakePicker({ "Claude Code": "claude" });
    const head = picker.menuHead("single");
    await assert.rejects(
      pinPickerToStub(api, picker.page, [{ kind: "menu", trigger: head.trigger }], provider, deps(env)),
      new RegExp(`\\b${provider}\\b`),
      JSON.stringify({ listing, env }),
    );
    assert.deepEqual(picker.clicks, []);
    assert.equal(head.head.value, "codex");
    assert.deepEqual(posts, []);
  }
});

test("an unconfigured or inactive stub throws rather than skips", async () => {
  for (const config of [deps({}), deps({ CODETERM_STUB_AGENT_CMD: "  " }), deps({ CODETERM_STUB_AGENT_CMD: STUB_CMD }, false)]) {
    const { api } = fakeApi([CLAUDE]);
    await assert.rejects(requireStubbedProvider(api, "claude", config), (error: unknown) => {
      assert.ok(error instanceof Error);
      assert.match(error.message, /\bclaude\b/);
      return true;
    });
  }
});

test("a head that does not take the stub fails naming the provider", async () => {
  const { api } = fakeApi([CLAUDE]);
  const picker = fakePicker({ Codex: "codex" });
  const head = picker.menuHead("single");
  await assert.rejects(
    pinPickerToStub(api, picker.page, [{ kind: "menu", trigger: head.trigger }], "claude", deps({ CODETERM_STUB_AGENT_CMD: STUB_CMD })),
    /claude must be installed and selectable/,
  );
  const loading = fakePicker({ "Claude Code": "claude" }).menuHead("single", "");
  await assert.rejects(
    pinPickerToStub(api, picker.page, [{ kind: "menu", trigger: loading.trigger }], "claude", deps({ CODETERM_STUB_AGENT_CMD: STUB_CMD })),
    /catalogue must load/,
  );
  await assert.rejects(
    pinPickerToStub(api, picker.page, [], "claude", deps({ CODETERM_STUB_AGENT_CMD: STUB_CMD })),
    /no picker head to pin to claude/,
  );
});
