import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { ExpandedComposer } from "./ExpandedComposer";
import { ComposerActions } from "./ComposerActions";
import { ExpandedDictationStatus } from "./voice/DictationStatus";
import { createDictationLevelChannel } from "./voice/dictationLevelModel";
import type { LiveDictation } from "./voice/useLiveDictation";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function makeDictation(phase: LiveDictation["phase"], cancel = vi.fn()): LiveDictation {
  const channel = createDictationLevelChannel();
  return {
    phase,
    error: null,
    setup: null,
    start: async () => {},
    stop: async () => {},
    cancel,
    finishThen: () => {},
    compositionEnded: () => {},
    dismissSetup: () => {},
    active: () => phase !== "idle",
    readLevel: channel.read,
    subscribeLevel: channel.subscribe,
  };
}

describe("ExpandedComposer layout bounds and dictation presence", () => {
  it("keeps bottom-anchored FormDialog free of marginTop and bounds maxHeight inside the viewport", async () => {
    const priorHeight = window.innerHeight;
    Object.defineProperty(window, "innerHeight", { value: 640, configurable: true });
    function Harness() {
      const anchor = useRef<HTMLDivElement>(null);
      return (
        <div ref={anchor}>
          <ExpandedComposer anchorRef={anchor} onClose={() => {}}>
            <textarea aria-label="Expanded draft" defaultValue="Line one" />
          </ExpandedComposer>
        </div>
      );
    }
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<Harness />));
      const dialog = document.body.querySelector<HTMLFormElement>('[role="dialog"].ct-expanded-composer')!;
      expect(dialog).not.toBeNull();
      expect(dialog.style.marginTop).toBe("");
      expect(dialog.style.bottom).not.toBe("");
      expect(dialog.style.maxHeight).not.toBe("");
      const maxHeightPx = Number.parseFloat(dialog.style.maxHeight);
      expect(maxHeightPx).toBeGreaterThanOrEqual(200);
      expect(maxHeightPx).toBeLessThanOrEqual(640);
      expect(dialog.querySelector('[data-testid="expanded-dictation-status"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
      Object.defineProperty(window, "innerHeight", { value: priorHeight, configurable: true });
    }
  });

  it("shows header dot+word status and bottom-bar dictation controls while dictating, and zero header clutter when idle", async () => {
    const cancel = vi.fn();
    function Harness({ phase }: { phase: LiveDictation["phase"] }) {
      const anchor = useRef<HTMLDivElement>(null);
      const dictation = makeDictation(phase, cancel);
      return (
        <div ref={anchor}>
          <ExpandedComposer
            anchorRef={anchor}
            onClose={() => {}}
            headerStatus={phase !== "idle" ? <ExpandedDictationStatus phase={phase} /> : undefined}
          >
            <ComposerActions
              expanded
              onAttach={() => {}}
              onSend={() => {}}
              onSendWithAttachments={() => {}}
              onError={() => {}}
              sendDisabled={false}
              hasText={false}
              dictation={dictation}
            >
              <textarea aria-label="Expanded draft" defaultValue="" />
            </ComposerActions>
          </ExpandedComposer>
        </div>
      );
    }
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<Harness phase="idle" />));
      let dialog = document.body.querySelector<HTMLFormElement>('[role="dialog"].ct-expanded-composer')!;
      expect(dialog.querySelector('[data-testid="expanded-dictation-status"]')).toBeNull();
      expect(dialog.querySelector('.ct-expanded-composer__dictation')).toBeNull();
      expect(dialog.querySelector('[data-testid="composer-attach-button"]')).not.toBeNull();
      expect(dialog.querySelector('[data-testid="composer-voice-button"]')).not.toBeNull();

      await act(async () => root.render(<Harness phase="listening" />));
      dialog = document.body.querySelector<HTMLFormElement>('[role="dialog"].ct-expanded-composer')!;
      const headerStatus = dialog.querySelector<HTMLElement>('[data-testid="expanded-dictation-status"]')!;
      expect(headerStatus).not.toBeNull();
      expect(headerStatus.textContent).toBe("Listening");
      expect(headerStatus.querySelector(".ct-expanded-dictation-dot")).not.toBeNull();

      const bottomDictation = dialog.querySelector<HTMLElement>(".ct-expanded-composer__dictation")!;
      expect(bottomDictation).not.toBeNull();
      expect(bottomDictation.querySelector('[data-testid="dictation-waveform"]')).not.toBeNull();
      const cancelBtn = bottomDictation.querySelector<HTMLButtonElement>('button[aria-label="Cancel dictation"]')!;
      expect(cancelBtn).not.toBeNull();
      await act(async () => cancelBtn.click());
      expect(cancel).toHaveBeenCalledOnce();
      expect(dialog.querySelector('[data-testid="composer-voice-button"]')).not.toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
