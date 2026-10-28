import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import type { ClipMessage } from "@codeterm/chat-core";
import { MessageActionsDot } from "./MessageActionsMenu";
import { ThreadRevertConfirmation } from "./ThreadRevertConfirmation";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const clip: ClipMessage = {
  id: "turn-two",
  role: "assistant",
  author: "Codex",
  source: "Second turn",
};

function HoverHarness() {
  const [open, setOpen] = useState(false);
  return <MessageActionsDot clip={clip} side="end" open={open} onOpenChange={setOpen}
    actions={{ quote: () => {}, ask: () => {}, select: () => {} }} />;
}

async function hoverFixture() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<HoverHarness />));
  const dot = host.querySelector<HTMLButtonElement>("button")!;
  const pointer = async (target: EventTarget, type: string) => {
    await act(async () => target.dispatchEvent(new PointerEvent(type, { bubbles: true, pointerType: "mouse" })));
  };
  await pointer(dot, "pointerover");
  const menu = () => document.querySelector<HTMLElement>('[role="menu"][aria-label="Message actions"]');
  expect(menu()).not.toBeNull();
  return { dot, menu, pointer, unmount: async () => { await act(async () => root.unmount()); host.remove(); } };
}

describe("message action hover wiring", () => {
  it.each(["escape", "outside press"])("opens on hover and dismisses on %s", async (dismiss) => {
    const view = await hoverFixture();
    try {
      if (dismiss === "escape") {
        await act(async () => document.body.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true, cancelable: true })));
      } else {
        const clicked = vi.fn();
        document.body.addEventListener("pointerdown", clicked, { once: true });
        await view.pointer(document.body, "pointerdown");
        expect(clicked).toHaveBeenCalledOnce();
      }
      expect(view.menu()).toBeNull();
    } finally { await view.unmount(); }
  });

  it("preserves keyboard focus and ignores hover dismissal after keyboard activation", async () => {
    const view = await hoverFixture();
    try {
      await act(async () => view.dot.dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowDown", bubbles: true, cancelable: true })));
      expect(document.activeElement?.getAttribute("role")).toBe("menuitem");
      await view.pointer(document.body, "pointermove");
      expect(view.menu()).not.toBeNull();
      await act(async () => document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true, cancelable: true })));
      expect(view.menu()).toBeNull();
      expect(document.activeElement).toBe(view.dot);
    } finally { await view.unmount(); }
  });
});

describe("thread history message actions", () => {
  it("exposes fork and revert menu items and calls each handler once", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const fork = vi.fn();
    const revert = vi.fn();
    try {
      await act(async () => {
        root.render(
          <MessageActionsDot
            clip={clip}
            side="end"
            open
            onOpenChange={() => {}}
            actions={{ quote: () => {}, ask: () => {}, select: () => {}, fork, revert }}
          />,
        );
      });
      const forkItem = document.querySelector<HTMLButtonElement>('[role="menuitem"][aria-label="Fork from here"]');
      const revertItem = document.querySelector<HTMLButtonElement>('[role="menuitem"][aria-label="Revert to here"]');
      expect(forkItem?.getAttribute("role")).toBe("menuitem");
      expect(revertItem?.getAttribute("role")).toBe("menuitem");
      await act(async () => forkItem?.click());
      await act(async () => revertItem?.click());
      expect(fork).toHaveBeenCalledTimes(1);
      expect(revert).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("requires confirmation before calling the selected revert handler", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const confirm = vi.fn();
    const cancel = vi.fn();
    try {
      await act(async () => {
        root.render(<ThreadRevertConfirmation turnId="turn-two" onConfirm={confirm} onCancel={cancel} />);
      });
      const dialog = host.querySelector<HTMLElement>('[role="dialog"][aria-label="Revert conversation history?"]');
      const confirmButton = dialog?.querySelector<HTMLButtonElement>('[data-testid="thread-revert-confirm-confirm"]');
      expect(dialog?.getAttribute("role")).toBe("dialog");
      await act(async () => confirmButton?.click());
      expect(confirm).toHaveBeenCalledTimes(1);
      expect(confirm).toHaveBeenCalledWith("turn-two");
      expect(cancel).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
