import { act, useMemo, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ToolCallBubble } from "../../../packages/chat-ui/src/ToolCallBubble";
import { manualPendingPrompt, promptMessagePart } from "../../lib/promptAnswers";
import type { Tab } from "../../lib/types";
import { PendingPromptCard } from "./PendingPromptCard";
import { clearPersistedChatScrollState, setPersistedChatScrollState } from "./chatViewport";
import { useMountedChatWindow } from "./useMountedChatWindow";
import { useChatScroll } from "./useChatScroll";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const SESSION = "permission-startup";
const rows = Array.from({ length: 80 }, (_, index) => ({ msg: { id: `m${index}` } }));
const prompt = { request_id: "opaque-request", question: "opaque-command; ".repeat(120),
  options: [{ label: "Once", value: "once" }, { label: "Always", value: "always" }, { label: "Reject", value: "reject" }] };
type State = Pick<Tab, "auto_approve" | "permission_auto_confirm" | "pending_prompt_kind" | "pending_prompt" | "remote_peer_id">;
let host: HTMLDivElement;
let root: Root;
let responses: string[];
let writes: number;
let followTail: { current: boolean };

function extent(element: HTMLElement): number {
  if (element.dataset.height) return Number(element.dataset.height);
  if (element.dataset.testid === "tab-pending-prompt") return 900;
  if (element.style.height) return Number.parseFloat(element.style.height) || 0;
  return Array.from(element.children).reduce((sum, child) => sum + extent(child as HTMLElement), 0);
}
const scroller = () => host.querySelector<HTMLDivElement>('[role="log"]')!;
function offset(element: HTMLElement): number {
  const parent = element.parentElement;
  if (!parent || element === scroller()) return 0;
  let top = offset(parent);
  for (const sibling of Array.from(parent.children)) {
    if (sibling === element) break;
    top += extent(sibling as HTMLElement);
  }
  return top;
}

function Harness({ state }: { state: State }) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [expanded, setExpanded] = useState(false);
  const mount = useMountedChatWindow({ rows, scrollRef, active: true, followTailRef: followTail, sessionKey: SESSION });
  const contentVersion = useMemo(() => ({ revision: mount.revision, state }), [mount.revision, state]);
  const scroll = useChatScroll({ scrollRef, sessionKey: SESSION, active: true, loading: false, messageCount: rows.length,
    contentVersion, prependVersion: rows[0].msg.id, followTailRef: followTail, onViewportChange: mount.syncViewport });
  const part = promptMessagePart({ supports_prompt_answers: true, supports_structured_prompts: true }, manualPendingPrompt(state));
  return <div role="log" onScroll={scroll.handleScroll} ref={element => {
    scrollRef.current = element;
    if (!element || Object.prototype.hasOwnProperty.call(element, "scrollHeight")) return;
    let top = 0;
    const maximum = () => Math.max(0, extent(element) - 500);
    Object.defineProperties(element, {
      clientHeight: { configurable: true, get: () => 500 },
      scrollHeight: { configurable: true, get: () => extent(element) },
      scrollTop: { configurable: true, get: () => Math.min(top, maximum()), set: (value: number) => {
        const next = Math.max(0, Math.min(value, maximum()));
        if (next !== top) writes++;
        top = next;
      } },
    });
  }}>
    <div>
      <div ref={mount.topSpacerRef} style={{ height: mount.topSpacer }} />
      <div ref={mount.rowsRef}>
        {rows.slice(mount.range.start, mount.range.end).map(({ msg }) => <div key={msg.id} data-chat-window-row={msg.id} data-height="100">
          <div data-message-id={msg.id}>
            {msg.id === "m79" ? <ToolCallBubble expanded={expanded} onToggle={setExpanded} message={{ id: msg.id, type: "tool_call", content: "",
              toolKind: "command", toolStatus: "running", toolInput: { command: "opaque-command" }, toolDetail: "opaque-command" }} /> : msg.id}
          </div>
        </div>)}
      </div>
      <div style={{ height: mount.range.bottom }} />
      {part && <PendingPromptCard key={part.id} pendingPrompt={part.prompt} onRespond={value => responses.push(value)} />}
    </div>
  </div>;
}

const render = async (state: State) => { await act(async () => root.render(<Harness state={state} />)); };
beforeEach(() => {
  responses = [];
  writes = 0;
  followTail = { current: true };
  clearPersistedChatScrollState(SESSION);
  vi.stubGlobal("ResizeObserver", undefined);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function(this: HTMLElement) {
    const top = this === scroller() ? 0 : offset(this) - scroller().scrollTop;
    const height = this === scroller() ? 500 : extent(this);
    return { top, bottom: top + height, height, left: 0, right: 800, width: 800, x: 0, y: top, toJSON: () => ({}) };
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  clearPersistedChatScrollState(SESSION);
});

it.each([[true, true], [true, false], [false, true], [false, false]])(
  "keeps a host-owned automatic permission out of transcript geometry (auto=%s, tab=%s)", async (auto_approve, permission_auto_confirm) => {
    const state: State = { auto_approve, permission_auto_confirm, pending_prompt: null };
    await render(state);
    const toggle = host.querySelector<HTMLButtonElement>('button[aria-expanded]')!;
    await act(async () => toggle.click());
    const command = host.querySelector('[data-message-id="m79"]')!;
    const before = { top: scroller().scrollTop, height: scroller().scrollHeight, rect: command.getBoundingClientRect().top };
    writes = 0;
    await render({ ...state, pending_prompt: prompt, pending_prompt_kind: "automatic_permission" });
    const automatic = auto_approve || permission_auto_confirm;
    expect(host.querySelector('[data-testid="tab-pending-prompt"]') === null).toBe(automatic);
    if (automatic) {
      expect(scroller().scrollTop).toBe(before.top);
      expect(scroller().scrollHeight).toBe(before.height);
      expect(command.getBoundingClientRect().top).toBe(before.rect);
      expect(writes).toBe(0);
    } else {
      expect(scroller().scrollHeight).toBeGreaterThan(before.height);
      await act(async () => host.querySelectorAll<HTMLButtonElement>('[data-testid="tab-pending-prompt-option"]').forEach(button => button.click()));
      expect(responses).toEqual(prompt.options.map(option => option.value));
    }
    await render(state);
    expect(scroller().scrollTop).toBe(before.top);
    expect(host.querySelector('[data-message-id="m79"]')).toBe(command);
    expect(host.querySelector('button[aria-expanded]')).toBe(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(followTail.current).toBe(true);
    expect(host.querySelectorAll('[data-chat-window-row]').length).toBeLessThan(rows.length);
  },
);

it.each(["structured", "permission", "directory_trust", "manual_update", "provider"] as const)(
  "keeps %s questions and refused automatic permissions answerable even with both flags enabled", async pending_prompt_kind => {
    await render({ auto_approve: true, permission_auto_confirm: true, pending_prompt_kind, pending_prompt: prompt });
    expect(host.querySelector('[data-testid="tab-pending-prompt-question"]')?.textContent).toBe(prompt.question);
    const buttons = host.querySelectorAll<HTMLButtonElement>('[data-testid="tab-pending-prompt-option"]');
    expect(buttons).toHaveLength(3);
    await act(async () => buttons[2].click());
    expect(responses).toEqual(["reject"]);
  },
);

it("keeps a detached reader anchored while an automatic permission appears and resolves", async () => {
  setPersistedChatScrollState(SESSION, { mode: "detached", scrollTop: 7700, anchor: { key: "m77", ordinal: 0, offset: 0 } });
  await render({ pending_prompt: null });
  expect(followTail.current).toBe(false);
  const anchor = host.querySelector('[data-message-id="m77"]')!;
  const before = { top: scroller().scrollTop, rect: anchor.getBoundingClientRect().top };
  writes = 0;
  await render({ auto_approve: true, pending_prompt_kind: "automatic_permission", pending_prompt: prompt });
  await render({ auto_approve: true, pending_prompt: null });
  expect(host.querySelector('[data-message-id="m77"]')).toBe(anchor);
  expect(anchor.getBoundingClientRect().top).toBe(before.rect);
  expect(scroller().scrollTop).toBe(before.top);
  expect(followTail.current).toBe(false);
  expect(writes).toBe(0);
});

it("preserves remote peer approvals even when both local automatic flags are enabled", async () => {
  await render({ remote_peer_id: "opaque-peer", auto_approve: true, permission_auto_confirm: true,
    pending_prompt_kind: "automatic_permission", pending_prompt: prompt });
  const buttons = host.querySelectorAll<HTMLButtonElement>('[data-testid="tab-pending-prompt-option"]');
  expect(buttons).toHaveLength(3);
  await act(async () => buttons[2].click());
  expect(responses).toEqual(["reject"]);
});
