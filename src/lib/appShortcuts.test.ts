import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  APP_SHORTCUT_ACTIONS,
  APP_SHORTCUT_DEFAULTS,
  dispatchAppShortcut,
  resolveAppShortcut,
  zoomedFontSize,
  type AppShortcutAction,
  type AppShortcutHandlers,
} from "./appShortcuts";

const CODES: Record<string, string> = { "=": "Equal", "-": "Minus", ",": "Comma", Tab: "Tab" };

/** The keydown a user produces for a chord on one platform; Ctrl means Cmd on mac unless it is Ctrl+Tab. */
function press(chord: string, mac: boolean, target: EventTarget | null = document.body) {
  const parts = chord.split("+");
  const key = parts[parts.length - 1];
  const code = CODES[key] ?? (/^[0-9]$/.test(key) ? `Digit${key}` : `Key${key.toUpperCase()}`);
  const ctrl = parts.includes("Ctrl");
  const literalCtrl = ctrl && key === "Tab";
  const event = {
    code,
    ctrlKey: ctrl && (!mac || literalCtrl),
    metaKey: ctrl && mac && !literalCtrl,
    shiftKey: parts.includes("Shift"),
    altKey: false,
    target,
    prevented: false,
    preventDefault() { event.prevented = true; },
    stopPropagation() {},
  };
  return event;
}

function recordingHandlers(calls: AppShortcutAction[], refuse: AppShortcutAction[] = []): AppShortcutHandlers {
  return Object.fromEntries(APP_SHORTCUT_ACTIONS.map((action) => [action, () => {
    calls.push(action);
    return refuse.includes(action) ? false : undefined;
  }])) as AppShortcutHandlers;
}

describe("app shortcut dispatcher", () => {
  it("covers every binding the daemon config declares", () => {
    const schema = readFileSync(resolve(process.cwd(), "src-tauri/crates/codeterm-types/src/config/schema.rs"), "utf8");
    const body = /pub struct KeybindingsConfig \{([\s\S]*?)\n\}/.exec(schema)?.[1] ?? "";
    const declared = [...body.matchAll(/pub (\w+): String/g)].map((match) => match[1]);
    expect(declared.length).toBeGreaterThan(0);
    for (const key of declared) expect(APP_SHORTCUT_ACTIONS).toContain(key);
  });

  for (const mac of [false, true]) {
    it(`runs exactly the advertised action for each default chord (${mac ? "mac" : "win/linux"})`, () => {
      for (const action of APP_SHORTCUT_ACTIONS) {
        const calls: AppShortcutAction[] = [];
        const event = press(APP_SHORTCUT_DEFAULTS[action], mac);
        expect(dispatchAppShortcut(event, {}, recordingHandlers(calls), mac)).toBe(action);
        expect(calls).toEqual([action]);
        expect(event.prevented).toBe(true);
      }
    });
  }

  it("cycles Tabs with Ctrl+Tab off mac", () => {
    expect(resolveAppShortcut(press("Ctrl+Tab", false), {}, false)).toBe("next_tab");
    expect(resolveAppShortcut(press("Ctrl+Shift+Tab", false), {}, false)).toBe("prev_tab");
  });

  it("follows a rebound chord and stops answering the old one", () => {
    const bindings = { new_tab: "Ctrl+Alt+N" };
    expect(resolveAppShortcut(press("Ctrl+T", false), bindings, false)).toBeNull();
    expect(resolveAppShortcut({ ...press("Ctrl+N", false), altKey: true }, bindings, false)).toBe("new_tab");
  });

  it("leaves Ctrl+letter to a focused terminal off mac, and keeps it on mac", () => {
    const xterm = document.createElement("div");
    xterm.className = "xterm";
    const input = document.createElement("textarea");
    xterm.append(input);
    const calls: AppShortcutAction[] = [];
    const winPress = press("Ctrl+T", false, input);
    expect(dispatchAppShortcut(winPress, {}, recordingHandlers(calls), false)).toBeNull();
    expect(winPress.prevented).toBe(false);
    expect(dispatchAppShortcut(press("Ctrl+T", true, input), {}, recordingHandlers(calls), true)).toBe("new_tab");
    expect(dispatchAppShortcut(press("Ctrl+Tab", false, input), {}, recordingHandlers(calls), false)).toBe("next_tab");
    expect(dispatchAppShortcut(press("Ctrl+Shift+V", false, input), {}, recordingHandlers(calls), false)).toBe("paste");
  });

  it("leaves copy and paste to an ordinary text field", () => {
    const field = document.createElement("input");
    const calls: AppShortcutAction[] = [];
    expect(dispatchAppShortcut(press("Ctrl+Shift+V", false, field), {}, recordingHandlers(calls), false)).toBeNull();
    expect(calls).toEqual([]);
  });

  it("keeps the native key when a handler has nothing to act on", () => {
    const calls: AppShortcutAction[] = [];
    const event = press("Ctrl+Shift+C", false);
    expect(dispatchAppShortcut(event, {}, recordingHandlers(calls, ["copy"]), false)).toBeNull();
    expect(calls).toEqual(["copy"]);
    expect(event.prevented).toBe(false);
  });

  it("zooms the font size within the Settings range", () => {
    expect(zoomedFontSize(14, "zoom_in")).toBe(15);
    expect(zoomedFontSize(28, "zoom_in")).toBe(28);
    expect(zoomedFontSize(8, "zoom_out")).toBe(8);
    expect(zoomedFontSize(20, "zoom_reset")).toBe(14);
  });
});
