import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { createMuxStateStore } from "../../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../../store/multiplexer/context";
import type { ExtendedState } from "../../../store/multiplexer/tree";
import { ComposerActions } from "../ComposerActions";
import { DictationPresence, DictationStatus } from "./DictationStatus";
import {
  createDictationLevelChannel,
  DICTATION_WAVEFORM_BARS,
  dictationWaveformBars,
  normalizeDictationRms,
  smoothDictationLevel,
} from "./dictationLevelModel";
import type { LiveDictation } from "./useLiveDictation";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("dictationLevelModel", () => {
  it("normalizes speech RMS above the noise floor and clamps cleanly", () => {
    expect(normalizeDictationRms(0)).toBe(0);
    expect(normalizeDictationRms(0.004)).toBe(0);
    expect(normalizeDictationRms(Number.NaN)).toBe(0);
    const quiet = normalizeDictationRms(0.025);
    const speech = normalizeDictationRms(0.09);
    const loud = normalizeDictationRms(0.5);
    expect(quiet).toBeGreaterThan(0.15);
    expect(speech).toBeGreaterThan(quiet);
    expect(loud).toBe(1);
  });

  it("attacks faster on speech than it decays into silence and settles to zero", () => {
    const attacked = smoothDictationLevel(0, 0.12);
    const decayed = smoothDictationLevel(attacked, 0);
    expect(attacked).toBeGreaterThan(0.3);
    expect(decayed).toBeLessThan(attacked);
    expect(decayed).toBeGreaterThan(0);
    let current = decayed;
    for (let i = 0; i < 20; i++) current = smoothDictationLevel(current, 0);
    expect(current).toBe(0);
  });

  it("produces five reactive bar heights and stays quiet on repeated silence", () => {
    const idleBars = dictationWaveformBars(0, 0);
    const activeBars = dictationWaveformBars(0.8, 3);
    expect(idleBars).toHaveLength(DICTATION_WAVEFORM_BARS);
    expect(activeBars).toHaveLength(DICTATION_WAVEFORM_BARS);
    expect(Math.max(...activeBars)).toBeGreaterThan(Math.max(...idleBars) + 0.3);

    const channel = createDictationLevelChannel();
    const listener = vi.fn();
    channel.subscribe(listener);
    channel.pushRms(0.001);
    channel.pushRms(0.002);
    expect(listener).not.toHaveBeenCalled();
    channel.pushRms(0.12);
    expect(listener).toHaveBeenCalledTimes(1);
    expect(channel.read().level).toBeGreaterThan(0.25);
    channel.reset();
    expect(channel.read().level).toBe(0);
  });
});

describe("DictationPresence and DictationStatus", () => {
  it("animates live waveform bars with sr-only status, zero parent re-renders, and quiet Cancel control", async () => {
    const channel = createDictationLevelChannel();
    const cancel = vi.fn();
    const dictation: LiveDictation = {
      phase: "listening",
      error: null,
      setup: null,
      start: async () => {},
      stop: async () => {},
      cancel,
      finishThen: () => {},
      compositionEnded: () => {},
      dismissSetup: () => {},
      dismissError: () => {},
      active: () => true,
      readLevel: channel.read,
      subscribeLevel: channel.subscribe,
    };
    let parentRenders = 0;
    function Parent() {
      parentRenders += 1;
      return <>
        <DictationStatus dictation={dictation} />
        <DictationPresence dictation={dictation} />
      </>;
    }
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<Parent />));
      expect(parentRenders).toBe(1);
      expect(host.querySelector('[role="alert"]')).toBeNull();
      const status = host.querySelector<HTMLElement>('[role="status"]')!;
      expect(status.classList.contains("sr-only")).toBe(true);
      expect(status.textContent).toBe("Listening");

      const waveform = host.querySelector<HTMLElement>('[data-testid="dictation-waveform"]')!;
      expect(waveform).not.toBeNull();
      expect(waveform.dataset.phase).toBe("listening");
      expect(waveform.dataset.silent).toBe("true");
      expect(waveform.querySelectorAll(".ct-dictation-bar")).toHaveLength(DICTATION_WAVEFORM_BARS);

      await act(async () => channel.pushRms(0.16));
      expect(parentRenders).toBe(1);
      expect(waveform.dataset.silent).toBe("false");
      expect(Number(waveform.dataset.level)).toBeGreaterThan(0.3);

      await act(async () => channel.reset());
      expect(waveform.dataset.silent).toBe("true");

      const cancelButton = host.querySelector<HTMLButtonElement>('button[aria-label="Cancel dictation"]')!;
      expect(cancelButton).not.toBeNull();
      expect(cancelButton.textContent).toBe("");
      await act(async () => cancelButton.click());
      expect(cancel).toHaveBeenCalledOnce();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("surfaces dictation errors as alerts while keeping starting/finishing phases on the same bar footprint", async () => {
    const channel = createDictationLevelChannel();
    const base: LiveDictation = {
      phase: "starting",
      error: "Microphone unavailable",
      setup: null,
      start: async () => {},
      stop: async () => {},
      cancel: () => {},
      finishThen: () => {},
      compositionEnded: () => {},
      dismissSetup: () => {},
      dismissError: () => {},
      active: () => true,
      readLevel: channel.read,
      subscribeLevel: channel.subscribe,
    };
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<>
        <DictationStatus dictation={base} />
        <DictationPresence dictation={base} />
      </>));
      expect(host.querySelector('[role="alert"]')?.textContent).toBe("Microphone unavailable");
      const waveform = host.querySelector<HTMLElement>('[data-testid="dictation-waveform"]')!;
      expect(waveform.dataset.phase).toBe("starting");
      expect(waveform.querySelectorAll(".ct-dictation-bar")).toHaveLength(DICTATION_WAVEFORM_BARS);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("renders Install dictation? as a floating glass bubble anchored to the mic shell with x and Escape dismissal, keeping the input card clean", async () => {
    const channel = createDictationLevelChannel();
    const dismissSetup = vi.fn();
    const dismissError = vi.fn();
    const state = {
      workspaces: [], tabs: {}, teams: [], notifications: [], tabStatuses: {}, toasts: [],
      activeTabIds: {}, generation: 1, pendingCloses: [], active_workspace_id: null, focused_tab_id: null,
    } as ExtendedState;
    const store = createMuxStateStore(() => state);
    const base: LiveDictation = {
      phase: "idle",
      error: null,
      setup: "No local transcriber installed.",
      start: async () => {},
      stop: async () => {},
      cancel: () => {},
      finishThen: () => {},
      compositionEnded: () => {},
      dismissSetup,
      dismissError,
      active: () => false,
      readLevel: channel.read,
      subscribeLevel: channel.subscribe,
    };
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(
        <MuxStateStoreContext.Provider value={store}>
          <ComposerActions
            onAttach={() => {}}
            onSend={() => {}}
            onSendWithAttachments={() => {}}
            onError={() => {}}
            sendDisabled={false}
            hasText={false}
            dictation={base}
          >
            <textarea aria-label="Message composer" defaultValue="" />
          </ComposerActions>
        </MuxStateStoreContext.Provider>,
      ));
      const inputCard = host.querySelector<HTMLElement>('[data-testid="composer-input-shell"]')!;
      const bubble = document.querySelector<HTMLElement>('[data-testid="dictation-setup-bubble"]')!;
      expect(bubble).not.toBeNull();
      expect(bubble.closest("[data-anchored-popover]")).not.toBeNull();
      expect(inputCard.contains(bubble)).toBe(false);
      expect(bubble.textContent).toContain("Install dictation?");
      expect(bubble.textContent).toContain("Set up with General");
      expect(bubble.textContent).not.toContain("Not now");

      const closeBtn = bubble.querySelector<HTMLButtonElement>('button[aria-label="Dismiss dictation setup"]')!;
      expect(closeBtn).not.toBeNull();
      await act(async () => closeBtn.click());
      expect(dismissSetup).toHaveBeenCalledTimes(1);

      await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true })));
      expect(dismissSetup).toHaveBeenCalledTimes(2);

      // Outside click also dismisses via AnchoredPopover.
      await act(async () => document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })));
      expect(dismissSetup).toHaveBeenCalledTimes(3);

      // Error bubble also anchors to the mic shell via AnchoredPopover with x dismiss and never inflates the input card.
      await act(async () => root.render(
        <MuxStateStoreContext.Provider value={store}>
          <ComposerActions
            onAttach={() => {}}
            onSend={() => {}}
            onSendWithAttachments={() => {}}
            onError={() => {}}
            sendDisabled={false}
            hasText={false}
            dictation={{ ...base, setup: null, error: "Microphone permission denied" }}
          >
            <textarea aria-label="Message composer" defaultValue="" />
          </ComposerActions>
        </MuxStateStoreContext.Provider>,
      ));
      const errorBubble = document.querySelector<HTMLElement>('[data-testid="dictation-error-bubble"]')!;
      expect(errorBubble).not.toBeNull();
      expect(errorBubble.closest("[data-anchored-popover]")).not.toBeNull();
      expect(inputCard.contains(errorBubble)).toBe(false);
      await act(async () => errorBubble.querySelector<HTMLButtonElement>('button[aria-label="Dismiss dictation error"]')!.click());
      expect(dismissError).toHaveBeenCalledOnce();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
