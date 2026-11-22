import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { __setInvokeForTest } from "../../../lib/bridge";
import { createMuxStateStore } from "../../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../../store/multiplexer/context";
import type { ExtendedState } from "../../../store/multiplexer/tree";
import { getByRole } from "../../../test/semanticRoles";
import { ComposerActions } from "../ComposerActions";
import { clearDictationSetupHandoff } from "./DictationSetupPrompt";
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

function deferred<T = void>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function makeMuxState(
  workspaces: ExtendedState["workspaces"],
  tabs: ExtendedState["tabs"],
  pendingCloses: string[] = [],
): ExtendedState {
  return {
    workspaces,
    tabs,
    teams: [],
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 1,
    pendingCloses,
    active_workspace_id: null,
    focused_tab_id: null,
  } as ExtendedState;
}

function localGeneralState(overrides: Partial<ExtendedState["tabs"][string]> = {}, tabId = "general-1", workspaceId = "ws-general") {
  return makeMuxState(
    [{ id: workspaceId, name: "General", tabs: [tabId], active_tab_id: tabId, cwd: null, protected: true }],
    {
      [tabId]: {
        id: tabId,
        pty_id: `pty-${tabId}`,
        tab_type: "Terminal",
        title: "General",
        cwd: null,
        protected: true,
        lifecycle_epoch: 1,
        session_binding_nonce: `nonce-${tabId}`,
        ...overrides,
      },
    },
  );
}

function makeSetupDictation(dismissSetup: () => void): LiveDictation {
  const channel = createDictationLevelChannel();
  return {
    phase: "idle",
    error: null,
    setup: "No local transcriber installed.",
    start: async () => {},
    stop: async () => {},
    cancel: () => {},
    finishThen: () => {},
    compositionEnded: () => {},
    dismissSetup,
    dismissError: () => {},
    active: () => false,
    readLevel: channel.read,
    subscribeLevel: channel.subscribe,
  };
}

function renderComposerWithSetup(
  store: ReturnType<typeof createMuxStateStore>,
  dictation: LiveDictation,
  draft = "Keep my draft",
) {
  return (
    <MuxStateStoreContext.Provider value={store}>
      <ComposerActions
        onAttach={() => {}}
        onSend={() => {}}
        onSendWithAttachments={() => {}}
        onError={() => {}}
        sendDisabled={false}
        hasText={draft.length > 0}
        dictation={dictation}
      >
        <textarea aria-label="Message composer" defaultValue={draft} />
      </ComposerActions>
    </MuxStateStoreContext.Provider>
  );
}

describe("DictationSetupPrompt General handoff", () => {
  afterEach(() => {
    __setInvokeForTest(null);
    clearDictationSetupHandoff();
    document.body.replaceChildren();
  });

  it("coalesces overlapping confirmations, preserves draft, and allows a later explicit request after completion", async () => {
    const sends: Array<{ tabId: unknown; text: unknown }> = [];
    const gates: Array<ReturnType<typeof deferred<unknown>>> = [];
    __setInvokeForTest(async (cmd, args) => {
      expect(cmd).toBe("chat_send_text");
      sends.push({ tabId: args?.tabId, text: args?.text });
      const gate = deferred<unknown>();
      gates.push(gate);
      return await gate.promise as never;
    });

    const focusEvents: Array<{ tabId?: string; workspaceId?: string }> = [];
    const onFocus = (event: Event) => {
      focusEvents.push((event as CustomEvent<{ tabId?: string; workspaceId?: string }>).detail);
    };
    window.addEventListener("codeterm:focus-tab", onFocus);

    const state = localGeneralState();
    const store = createMuxStateStore(() => state);
    const dismissFirst = vi.fn();
    const dismissSecond = vi.fn();
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(renderComposerWithSetup(store, makeSetupDictation(dismissFirst), "Unsent composer draft")));
      const button = getByRole(document.body, "button", "Set up with General") as HTMLButtonElement;

      await act(async () => {
        button.click();
        button.click();
      });
      expect(sends).toHaveLength(1);
      expect(sends[0].tabId).toBe("general-1");
      expect(button.disabled).toBe(true);
      expect(focusEvents).toHaveLength(0);
      expect(dismissFirst).not.toHaveBeenCalled();

      await act(async () => gates[0].resolve({ send_id: "s1", tab_id: "general-1", state: "delivered", error: null, ledger_id: "l1", sequence: 1, retained_from: 1 }));
      expect(focusEvents).toEqual([{ tabId: "general-1", workspaceId: "ws-general" }]);
      expect(dismissFirst).toHaveBeenCalledTimes(1);
      const draft = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message composer"]')!;
      expect(draft.value).toBe("Unsent composer draft");

      await act(async () => root.render(renderComposerWithSetup(store, { ...makeSetupDictation(dismissFirst), setup: null }, "Unsent composer draft")));
      await act(async () => root.render(renderComposerWithSetup(store, makeSetupDictation(dismissSecond), "Unsent composer draft")));
      const retryButton = getByRole(document.body, "button", "Set up with General") as HTMLButtonElement;
      await act(async () => retryButton.click());
      expect(sends).toHaveLength(2);
      expect(sends[1].tabId).toBe("general-1");

      await act(async () => gates[1].resolve({ send_id: "s2", tab_id: "general-1", state: "held", error: null, ledger_id: "l1", sequence: 2, retained_from: 1 }));
      expect(focusEvents).toEqual([
        { tabId: "general-1", workspaceId: "ws-general" },
        { tabId: "general-1", workspaceId: "ws-general" },
      ]);
      expect(dismissSecond).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener("codeterm:focus-tab", onFocus);
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("coalesces remount while pending without duplicating requests and never applies stale focus after unmount", async () => {
    const sends: Array<{ tabId: unknown }> = [];
    const gates: Array<ReturnType<typeof deferred<unknown>>> = [];
    __setInvokeForTest(async (cmd, args) => {
      expect(cmd).toBe("chat_send_text");
      sends.push({ tabId: args?.tabId });
      const gate = deferred<unknown>();
      gates.push(gate);
      return await gate.promise as never;
    });

    const focusEvents: Array<{ tabId?: string; workspaceId?: string }> = [];
    const onFocus = (event: Event) => {
      focusEvents.push((event as CustomEvent<{ tabId?: string; workspaceId?: string }>).detail);
    };
    window.addEventListener("codeterm:focus-tab", onFocus);

    const state = localGeneralState();
    const store = createMuxStateStore(() => state);
    const dismissMount1 = vi.fn();
    const dismissMount2 = vi.fn();
    const dismissMount3 = vi.fn();
    const host1 = document.createElement("div");
    const host2 = document.createElement("div");
    document.body.append(host1, host2);
    const root1 = createRoot(host1);
    const root2 = createRoot(host2);
    try {
      await act(async () => root1.render(renderComposerWithSetup(store, makeSetupDictation(dismissMount1))));
      await act(async () => getByRole(document.body, "button", "Set up with General").click());
      expect(sends).toHaveLength(1);

      await act(async () => root1.unmount());
      await act(async () => root2.render(renderComposerWithSetup(store, makeSetupDictation(dismissMount2))));
      const remountedButton = getByRole(document.body, "button", "Set up with General") as HTMLButtonElement;
      await act(async () => remountedButton.click());
      expect(sends).toHaveLength(1);
      expect(remountedButton.disabled).toBe(true);

      await act(async () => gates[0].resolve(undefined));
      expect(focusEvents).toEqual([{ tabId: "general-1", workspaceId: "ws-general" }]);
      expect(dismissMount1).not.toHaveBeenCalled();
      expect(dismissMount2).toHaveBeenCalledTimes(1);

      await act(async () => root2.render(renderComposerWithSetup(store, makeSetupDictation(dismissMount3))));
      await act(async () => getByRole(document.body, "button", "Set up with General").click());
      expect(sends).toHaveLength(2);
      await act(async () => root2.unmount());
      await act(async () => gates[1].resolve(undefined));
      expect(focusEvents).toHaveLength(1);
      expect(dismissMount3).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("codeterm:focus-tab", onFocus);
      await act(async () => {
        root1.unmount();
        root2.unmount();
      });
      host1.remove();
      host2.remove();
    }
  });

  it("refuses foreign General tabs and prevents stale focus when local General authority is replaced mid-flight", async () => {
    const sends: Array<{ tabId: unknown }> = [];
    const gates: Array<ReturnType<typeof deferred<unknown>>> = [];
    __setInvokeForTest(async (cmd, args) => {
      expect(cmd).toBe("chat_send_text");
      sends.push({ tabId: args?.tabId });
      const gate = deferred<unknown>();
      gates.push(gate);
      return await gate.promise as never;
    });

    const focusEvents: Array<{ tabId?: string; workspaceId?: string }> = [];
    const onFocus = (event: Event) => {
      focusEvents.push((event as CustomEvent<{ tabId?: string; workspaceId?: string }>).detail);
    };
    window.addEventListener("codeterm:focus-tab", onFocus);

    let currentState = makeMuxState(
      [{ id: "ws-remote", name: "Peer General", tabs: ["mesh:peer-a/general"], active_tab_id: "mesh:peer-a/general", cwd: null, protected: true }],
      {
        "mesh:peer-a/general": {
          id: "mesh:peer-a/general",
          pty_id: null,
          tab_type: "Terminal",
          title: "General",
          cwd: null,
          protected: true,
          remote_peer_id: "peer-a",
          remote_tab_id: "general",
        },
      },
    );
    const store = createMuxStateStore(() => currentState);
    const dismissFirst = vi.fn();
    const dismissSecond = vi.fn();
    const host1 = document.createElement("div");
    const host2 = document.createElement("div");
    document.body.append(host1, host2);
    const root1 = createRoot(host1);
    const root2 = createRoot(host2);
    try {
      await act(async () => root1.render(renderComposerWithSetup(store, makeSetupDictation(dismissFirst))));
      await act(async () => getByRole(document.body, "button", "Set up with General").click());
      expect(sends).toHaveLength(0);
      expect(focusEvents).toHaveLength(0);
      expect(dismissFirst).not.toHaveBeenCalled();
      expect(document.querySelector('[role="alert"]')?.textContent).toContain("General Agent is not running");

      currentState = localGeneralState({ lifecycle_epoch: 1, pty_id: "pty-1" }, "general-1", "ws-general");
      await act(async () => getByRole(document.body, "button", "Set up with General").click());
      expect(sends).toEqual([{ tabId: "general-1" }]);

      currentState = localGeneralState({ lifecycle_epoch: 2, pty_id: "pty-2" }, "general-2", "ws-general");
      await act(async () => root2.render(renderComposerWithSetup(store, makeSetupDictation(dismissSecond))));
      const buttons = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).filter(
        candidate => candidate.textContent?.trim() === "Set up with General",
      );
      expect(buttons).toHaveLength(2);
      await act(async () => buttons[1].click());
      expect(sends).toEqual([{ tabId: "general-1" }, { tabId: "general-2" }]);

      await act(async () => gates[0].resolve({ send_id: "s1", tab_id: "general-1", state: "delivered", error: null, ledger_id: "l1", sequence: 1, retained_from: 1 }));
      expect(focusEvents).toHaveLength(0);
      expect(dismissFirst).not.toHaveBeenCalled();
      expect(document.querySelector('[role="alert"]')?.textContent).toContain("General Agent changed");

      await act(async () => gates[1].resolve({ send_id: "s2", tab_id: "general-2", state: "delivered", error: null, ledger_id: "l1", sequence: 1, retained_from: 1 }));
      expect(focusEvents).toEqual([{ tabId: "general-2", workspaceId: "ws-general" }]);
      expect(dismissSecond).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener("codeterm:focus-tab", onFocus);
      await act(async () => {
        root1.unmount();
        root2.unmount();
      });
      host1.remove();
      host2.remove();
    }
  });

  it("keeps refusal, failed delivery, and uncertain delivery truthful without automatic replay or dismissal", async () => {
    const outcomes: Array<() => Promise<unknown>> = [
      () => Promise.reject(new Error("General refused input")),
      () => Promise.resolve({ send_id: "s1", tab_id: "general-1", state: "failed", error: "General session unavailable", ledger_id: "l1", sequence: 1, retained_from: 1 }),
      () => Promise.resolve({ send_id: "s2", tab_id: "general-1", state: "submit_unconfirmed", error: null, ledger_id: "l1", sequence: 2, retained_from: 1 }),
    ];
    let calls = 0;
    __setInvokeForTest(async (cmd) => {
      expect(cmd).toBe("chat_send_text");
      const next = outcomes[calls++];
      if (!next) throw new Error("Unexpected automatic replay");
      return await next() as never;
    });

    const focusEvents: Event[] = [];
    const onFocus = (event: Event) => focusEvents.push(event);
    window.addEventListener("codeterm:focus-tab", onFocus);

    const state = localGeneralState();
    const store = createMuxStateStore(() => state);
    const dismissSetup = vi.fn();
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(renderComposerWithSetup(store, makeSetupDictation(dismissSetup))));

      await act(async () => getByRole(document.body, "button", "Set up with General").click());
      expect(calls).toBe(1);
      expect(document.querySelector('[role="alert"]')?.textContent).toBe("General refused input");
      expect(dismissSetup).not.toHaveBeenCalled();
      expect(focusEvents).toHaveLength(0);

      await act(async () => getByRole(document.body, "button", "Set up with General").click());
      expect(calls).toBe(2);
      expect(document.querySelector('[role="alert"]')?.textContent).toBe("General session unavailable");
      expect(dismissSetup).not.toHaveBeenCalled();
      expect(focusEvents).toHaveLength(0);

      await act(async () => getByRole(document.body, "button", "Set up with General").click());
      expect(calls).toBe(3);
      expect(document.querySelector('[role="alert"]')?.textContent).toContain("unconfirmed");
      expect(dismissSetup).not.toHaveBeenCalled();
      expect(focusEvents).toHaveLength(0);
    } finally {
      window.removeEventListener("codeterm:focus-tab", onFocus);
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
