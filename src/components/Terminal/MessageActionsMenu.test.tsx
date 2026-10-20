import { act } from "react";
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
