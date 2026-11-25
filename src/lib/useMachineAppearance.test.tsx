import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { useMachineAppearance } from "./useMachineAppearance";
import { DEFAULT_THEME } from "./themeCss";
import { setWorkspaceBrowseTarget } from "./workspaceBrowseTarget";
import type { AppConfig } from "./types";
import type { MachineAppearance } from "./chatWallpaperClient";
import type { AppearanceProposal } from "./appearance";
import { APPEARANCE_CHANGED, type AppearanceCommit } from "./appearanceClient";
import type { PeerEvent } from "./peerEventRail";
import { bindAppearancePress, indicateAppearancePress } from "./appearanceMotion";
import { useAppearanceMotion } from "../hooks/useAppearanceMotion";

const { read, observe, listenCommit, listeners } = vi.hoisted(() => {
  const listeners = new Map<string, Set<(event: PeerEvent) => void>>();
  return {
    read: vi.fn(),
    observe: vi.fn((peerId: string, receive: (event: PeerEvent) => void) => {
      const set = listeners.get(peerId) ?? new Set();
      set.add(receive);
      listeners.set(peerId, set);
      return () => {
        set.delete(receive);
        if (!set.size) listeners.delete(peerId);
      };
    }),
    listenCommit: vi.fn(async (_receive: (commit: AppearanceCommit) => void) => () => {}),
    listeners,
  };
});
vi.mock("./chatWallpaperClient", () => ({ readMachineAppearance: read, readWallpaperAsset: vi.fn() }));
vi.mock("./peerEventRail", () => ({ peerEventRail: { observe } }));
vi.mock("./appearanceClient", () => ({
  APPEARANCE_CHANGED: "codeterm:appearance-changed",
  listenAppearanceCommit: listenCommit,
}));

function emitPeer(peerId: string, event: PeerEvent) {
  for (const receive of [...(listeners.get(peerId) ?? [])]) receive(event);
}

it("keeps the newest requested machine when older responses arrive later", async () => {
  const queued = new Map<string, (look: MachineAppearance) => void>();
  read.mockImplementation((id: string) => new Promise(resolve => queued.set(id, resolve)));
  const rootNode = document.createElement("div");
  document.body.append(rootNode);
  const root = createRoot(rootNode);
  const config = { theme: DEFAULT_THEME, chat_wallpaper: { background: { pattern: "dots" } } } as AppConfig;
  function Fixture() {
    const selected = useMachineAppearance(config, true);
    return <output>{selected.ready ? selected.appearance.wallpaper?.background.pattern : "loading"}</output>;
  }
  try {
    await act(async () => root.render(<Fixture />));
    expect(rootNode.textContent).toBe("dots");
    await act(async () => setWorkspaceBrowseTarget({ id: "older", name: "Older" }));
    await act(async () => setWorkspaceBrowseTarget({ id: "newer", name: "Newer" }));
    expect(rootNode.textContent).toBe("loading");
    await act(async () => queued.get("newer")!({ theme: DEFAULT_THEME, wallpaper: { background: { pattern: "space" } } }));
    await act(async () => queued.get("older")!({ theme: DEFAULT_THEME, wallpaper: { background: { pattern: "none" } } }));
    expect(rootNode.textContent).toBe("space");
    await act(async () => setWorkspaceBrowseTarget(null));
    expect(rootNode.textContent).toBe("dots");
  } finally {
    await act(async () => root.unmount());
    setWorkspaceBrowseTarget(null);
    rootNode.remove();
  }
});


it("applies a config update and an explicit machine switch inside one transition each", async () => {
  const base = { theme: DEFAULT_THEME, wallpaper: null, tuning: null };
  const theme = { ...DEFAULT_THEME, name: "External theme", background: "#101a23" };
  const { background, ...colors } = theme;
  const reorderedTheme = { background, ...colors };
  const peerWallpaper = { background: { pattern: "dots" as const } };
  const peerLook = { ...base, theme, wallpaper: peerWallpaper };
  let remoteLook: MachineAppearance = base;
  read.mockImplementation(async (id: string) => id === "different" ? peerLook : id === "initial" ? remoteLook : base);
  const rootNode = document.createElement("main");
  document.body.append(rootNode);
  const root = createRoot(rootNode);
  const config = { theme: DEFAULT_THEME } as AppConfig;
  const transitions: { update: () => void; finish: () => void; skip: ReturnType<typeof vi.fn> }[] = [];
  const start = vi.fn((update: () => void) => {
    let finish!: () => void;
    const finished = new Promise<void>(resolve => { finish = resolve; });
    const skip = vi.fn(finish);
    transitions.push({ update, finish, skip });
    return { ready: Promise.resolve(), finished, skipTransition: skip };
  });
  const oldStart = Object.getOwnPropertyDescriptor(document, "startViewTransition");
  Object.defineProperty(document, "startViewTransition", { configurable: true, value: start });
  const motionPreferences = vi.spyOn(window, "matchMedia");
  const motion = () => document.documentElement.dataset.appearanceMotion;
  const origin = () => document.documentElement.style.getPropertyValue("--ct-appearance-origin");
  const radius = () => document.documentElement.style.getPropertyValue("--ct-appearance-radius");
  function Fixture() {
    const selected = useMachineAppearance(config, true);
    const { look } = useAppearanceMotion(selected);
    return <div className="ct-chat-surface">
      <output data-appearance-proposal="proposal">{look.theme.name} · {look.wallpaper?.background.pattern ?? "none"}</output>
      <button type="button" aria-label="Apply" />
    </div>;
  }
  try {
    observe.mockClear();
    listenCommit.mockClear();
    await act(async () => root.render(<Fixture />));
    await act(async () => setWorkspaceBrowseTarget({ id: "initial", name: "Initial" }));
    expect(start).not.toHaveBeenCalled();

    const surface = rootNode.querySelector(".ct-chat-surface")!;
    const card = rootNode.querySelector("[data-appearance-proposal]")!;
    vi.spyOn(surface, "getBoundingClientRect").mockReturnValue(new DOMRect(100, 100, 200, 200));
    vi.spyOn(card, "getBoundingClientRect").mockReturnValue(new DOMRect(110, 110, 100, 50));
    const [receiveCommit] = listenCommit.mock.calls[listenCommit.mock.calls.length - 1];
    remoteLook = { ...base, theme };
    await act(async () => {
      receiveCommit({ event: "cli-event", machine: "initial", proposal: "proposal", undo: false });
      emitPeer("initial", { type: "config-changed" });
    });
    expect(start).toHaveBeenCalledTimes(1);
    expect(motion()).toBe("wave");
    expect(origin()).toBe("200px 200px");
    expect(parseFloat(radius())).toBeGreaterThan(0);
    expect(document.documentElement.hasAttribute("data-appearance-content-fade")).toBe(false);
    expect(rootNode.textContent).toBe(`${DEFAULT_THEME.name} · none`);
    await act(async () => {
      transitions[0].update();
      expect(rootNode.textContent).toBe("External theme · none");
      transitions[0].finish();
    });
    expect(motion()).toBeUndefined();
    expect(origin()).toBe("");
    expect(radius()).toBe("");
    remoteLook = { ...base, theme: reorderedTheme };
    await act(async () => emitPeer("initial", { type: "config-changed" }));
    expect(start).toHaveBeenCalledTimes(1);

    const applyButton = rootNode.querySelector("button")!;
    vi.spyOn(applyButton, "getBoundingClientRect").mockReturnValue(new DOMRect(116, 120, 80, 28));
    const nextWallpaper = { background: { pattern: "space" as const } };
    const appliedLook = { ...base, theme, wallpaper: nextWallpaper };
    const receipt: AppearanceProposal = {
      id: "proposal",
      machine: { id: "initial", name: "Initial" },
      rationale: "Applied from Lab",
      look: appliedLook,
      previous: { ...base, theme },
      state: "applied",
      applied_at: 1,
      can_undo: true,
      contrast: [],
    };
    const readsBeforeApply = read.mock.calls.length;
    indicateAppearancePress(applyButton);
    bindAppearancePress(applyButton, "proposal", appliedLook, "initial");
    await act(async () => {
      window.dispatchEvent(new CustomEvent(APPEARANCE_CHANGED, {
        detail: { target: { machine: "initial", peer: "initial" }, receipt },
      }));
    });
    expect(read.mock.calls.length).toBe(readsBeforeApply);
    expect(start).toHaveBeenCalledTimes(2);
    expect(motion()).toBe("wave");
    expect(origin()).toBe("156px 134px");
    await act(async () => {
      transitions[1].update();
      expect(rootNode.textContent).toBe("External theme · space");
      transitions[1].finish();
    });

    // Late duplicate peer-rail events for the same applied commit must not re-animate.
    remoteLook = appliedLook;
    await act(async () => {
      receiveCommit({ event: "late-rail", machine: "initial", proposal: "proposal", undo: false });
      emitPeer("initial", { type: "config-changed" });
    });
    expect(start).toHaveBeenCalledTimes(2);

    // Remote Undo via APPEARANCE_CHANGED updates live and fades instead of waving.
    const undoneReceipt: AppearanceProposal = { ...receipt, state: "undone", can_undo: false };
    await act(async () => {
      window.dispatchEvent(new CustomEvent(APPEARANCE_CHANGED, {
        detail: { target: { machine: "initial", peer: "initial" }, receipt: undoneReceipt },
      }));
    });
    expect(start).toHaveBeenCalledTimes(3);
    expect(motion()).toBe("fade");
    await act(async () => {
      transitions[2].update();
      expect(rootNode.textContent).toBe("External theme · none");
      transitions[2].finish();
    });

    // External change made elsewhere on the peer during reconnect reconciles live.
    remoteLook = appliedLook;
    await act(async () => emitPeer("initial", { type: "reconcile" }));
    expect(start).toHaveBeenCalledTimes(4);
    expect(motion()).toBe("wave");
    await act(async () => {
      transitions[3].update();
      expect(rootNode.textContent).toBe("External theme · space");
      transitions[3].finish();
    });

    await act(async () => setWorkspaceBrowseTarget({ id: "different", name: "Different" }, { x: 28, y: 90 }));
    expect(start).toHaveBeenCalledTimes(5);
    expect(origin()).toBe("28px 90px");
    expect(rootNode.textContent).toBe("External theme · space");
    await act(async () => {
      transitions[4].update();
      expect(rootNode.textContent).toBe("External theme · dots");
      transitions[4].finish();
    });

    await act(async () => setWorkspaceBrowseTarget({ id: "same", name: "Same look" }));
    expect(start).toHaveBeenCalledTimes(5);
    await act(async () => setWorkspaceBrowseTarget(null, { x: 30, y: 20 }));
    expect(start).toHaveBeenCalledTimes(5);
    await act(async () => setWorkspaceBrowseTarget({ id: "different", name: "Different" }));
    expect(start).toHaveBeenCalledTimes(5);

    const preferences = window.matchMedia("(prefers-reduced-motion: reduce)");
    motionPreferences.mockReturnValue({ ...preferences, matches: true });
    await act(async () => setWorkspaceBrowseTarget(null, { x: 30, y: 20 }));
    expect(start).toHaveBeenCalledTimes(6);
    expect(motion()).toBe("fade");

    await act(async () => {
      transitions[5].update();
      window.dispatchEvent(new Event("codeterm-peer-switch-reset"));
    });
    expect(transitions[5].skip).toHaveBeenCalledTimes(1);
    expect(motion()).toBeUndefined();
    expect(rootNode.textContent).toBe(`${DEFAULT_THEME.name} · none`);
  } finally {
    await act(async () => root.unmount());
    setWorkspaceBrowseTarget(null);
    if (oldStart) Object.defineProperty(document, "startViewTransition", oldStart);
    else Reflect.deleteProperty(document, "startViewTransition");
    motionPreferences.mockRestore();
    rootNode.remove();
  }
});
