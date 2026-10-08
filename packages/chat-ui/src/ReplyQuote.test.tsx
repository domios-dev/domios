import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { formatAttachmentMarker, type NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "./MessageBubble";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  host?.remove();
  host = null;
});

async function render(content: string, onJumpToMessage?: (id: string) => void) {
  const message: NormalizedChatMessage = { id: "u9", type: "user", content };
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<MessageBubble message={message} onJumpToMessage={onJumpToMessage} />));
  return host;
}

const REPLY = formatAttachmentMarker("/tmp/ct/quote-codex.md", "quote", "Codex · 14:05", { ref: "a1", excerpt: "The build is green now." });

it("renders the reply at the top of the bubble and jumps to the quoted message once", async () => {
  const jump = vi.fn();
  const view = await render(`${REPLY}\nwhy did it fail before?`, jump);
  const block = view.querySelector<HTMLElement>('[data-testid="reply-quote"]');
  expect(block?.getAttribute("aria-label")).toBe("In reply to Codex · 14:05");
  expect(block?.textContent).toContain("The build is green now.");
  expect(view.textContent).toContain("why did it fail before?");
  expect(view.textContent).not.toContain("<domios");
  expect(view.querySelector('[data-testid="attachment-capsule"]')).toBeNull();
  await act(async () => block?.querySelector("button")?.click());
  expect(jump).toHaveBeenCalledTimes(1);
  expect(jump).toHaveBeenCalledWith("a1");
});

it("shows the reply without a jump control when nothing can scroll", async () => {
  const view = await render(REPLY);
  expect(view.querySelector('[data-testid="reply-quote"]')).not.toBeNull();
  expect(view.querySelector('[data-testid="reply-quote"] button')).toBeNull();
});
