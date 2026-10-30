// Run: npx tsx packages/chat-ui/src/blocks/ChoiceBlock.test.tsx

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { ChoiceBlockState } from "./choiceBlockModel";

const tests: Array<[string, () => Promise<void>]> = [];
function test(name: string, run: () => Promise<void>) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? testWindow : testWindow[key], configurable: true });
}
testWindow.matchMedia = (() => ({
  matches: false, media: "", onchange: null,
  addEventListener: () => {}, removeEventListener: () => {},
  addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
})) as unknown as typeof testWindow.matchMedia;

/** A host whose transcript the test controls; a send appends the reply like the composer's optimistic row. */
function fakeChoiceHost() {
  const replies = new Map<string, string>();
  const listeners = new Set<() => void>();
  const sent: string[] = [];
  let release: () => void = () => {};
  const notify = () => { for (const listener of listeners) listener(); };
  const cache = new Map<string, ChoiceBlockState>();
  const host = {
    binding: {
      subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
      read(id: string): ChoiceBlockState {
        const reply = replies.get(id) ?? null;
        const cached = cache.get(id);
        if (cached && cached.reply === reply) return cached;
        const next = { reply };
        cache.set(id, next);
        return next;
      },
      refresh() {},
    },
    choose(id: string, label: string) {
      sent.push(label);
      return new Promise<boolean>((resolve) => {
        release = () => {
          replies.set(id, label);
          notify();
          resolve(true);
        };
      });
    },
  };
  return { host, sent, finish: () => release(), reply: (id: string, text: string) => { replies.set(id, text); notify(); } };
}

(async () => {
  const { RichMarkdown } = await import("../MarkdownBody");
  const { ChatBlockHostProvider, ChatBlockMessageProvider } = await import("./ChatBlockHost");
  type Host = Parameters<typeof ChatBlockHostProvider>[0]["value"];
  const menuButtons: Host["renderMenu"] = ({ items }) => items.map((item) => <button key={item.label} type="button" aria-label={item.label} onClick={item.onSelect} />);

  async function withRender(text: string, value: Host, messageId: string | null, run: (element: HTMLDivElement) => Promise<void>) {
    const element = document.createElement("div");
    document.body.appendChild(element);
    const root = createRoot(element);
    const body = <RichMarkdown text={text} />;
    try {
      await act(async () => root.render(
        <ChatBlockHostProvider value={value}>
          {messageId ? <ChatBlockMessageProvider messageId={messageId}>{body}</ChatBlockMessageProvider> : body}
        </ChatBlockHostProvider>,
      ));
      await run(element);
    } finally {
      await act(async () => root.unmount());
      element.remove();
    }
  }
  const option = (element: HTMLElement, label: string) => element.querySelector(`[data-choice-option="${label}"]`) as HTMLButtonElement;
  const status = (element: HTMLElement) => element.querySelector('[data-testid="chat-block-status"]')?.textContent;
  const TEXT = "Deploy to production now?\n@ds-choice Ship it | Hold | Ask me later";

  test("renders_options_under_a_lead_sentence_without_sending", async () => {
    const { host, sent } = fakeChoiceHost();
    await withRender(TEXT, { choice: host }, "m1", async (element) => {
      assert(element.querySelector('[data-chat-block="choice"]'), "choice block missing");
      assert(element.textContent?.includes("Deploy to production now?"), "lead sentence lost");
      assert(["Ship it", "Hold", "Ask me later"].every((label) => option(element, label) && !option(element, label).disabled), "options not tappable");
      assert(status(element) === "Waiting for you", "open status");
      assert(sent.length === 0, "rendering sent a reply");
    });
  });

  test("a_double_tap_sends_once_and_the_reply_marks_the_choice", async () => {
    const { host, sent, finish } = fakeChoiceHost();
    await withRender(TEXT, { choice: host }, "m1", async (element) => {
      await act(async () => {
        option(element, "Hold").click();
        option(element, "Hold").click();
        option(element, "Ship it").click();
      });
      assert(sent.length === 1 && sent[0] === "Hold", `sent ${JSON.stringify(sent)}`);
      assert(option(element, "Ship it").disabled && option(element, "Ask me later").disabled, "other options stay enabled while sending");
      await act(async () => finish());
      assert(status(element) === "Sent", "status did not follow the transcript");
      assert(option(element, "Hold").getAttribute("aria-pressed") === "true", "chosen option not marked");
      assert(option(element, "Ship it").getAttribute("aria-pressed") === "false" && option(element, "Ship it").disabled, "others not disabled after the choice");
      await act(async () => option(element, "Ship it").click());
      assert(sent.length === 1, "a second choice was sent");
    });
  });

  test("a_typed_reply_closes_the_question", async () => {
    const { host, sent, reply } = fakeChoiceHost();
    await withRender(TEXT, { choice: host }, "m1", async (element) => {
      await act(async () => reply("m1", "let me think"));
      assert(status(element) === "Answered in chat", "typed reply not reflected");
      assert(option(element, "Hold").disabled, "options open after a typed reply");
      await act(async () => option(element, "Hold").click());
      assert(sent.length === 0, "clicked a closed question");
    });
  });

  test("an_earlier_answer_shows_on_mount", async () => {
    const { host, reply } = fakeChoiceHost();
    reply("m1", "ship it");
    await withRender(TEXT, { choice: host }, "m1", async (element) => {
      assert(option(element, "Ship it").getAttribute("aria-pressed") === "true", "earlier answer not shown");
    });
  });

  test("outside_its_chat_the_choice_cannot_send_and_cannot_be_pinned", async () => {
    const { host, sent } = fakeChoiceHost();
    const pins: string[] = [];
    await withRender(TEXT, { choice: host, renderMenu: menuButtons, pin: (source) => pins.push(source) }, null, async (element) => {
      assert(option(element, "Hold").disabled, "choice without its message must not send");
      await act(async () => option(element, "Hold").click());
      assert(sent.length === 0, "sent without a message scope");
      assert(!element.querySelector('[aria-label^="Pin "]'), "choice offers Pin");
    });
  });

  test("a_failed_send_reopens_the_options_and_says_why", async () => {
    const open: ChoiceBlockState = { reply: null };
    const failing = {
      binding: { subscribe: () => () => {}, read: () => open, refresh() {} },
      choose: async () => { throw new Error("tab closed"); },
    };
    await withRender(TEXT, { choice: failing }, "m1", async (element) => {
      await act(async () => option(element, "Hold").click());
      assert(element.querySelector('[role="alert"]')?.textContent?.includes("tab closed"), "failure not shown");
      assert(!option(element, "Hold").disabled, "options stay closed after a failed send");
    });
  });

  let failed = 0;
  for (const [name, run] of tests) {
    try {
      await run();
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failed++;
      console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
    }
  }
  console.log(`chat-ui/blocks/ChoiceBlock tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
