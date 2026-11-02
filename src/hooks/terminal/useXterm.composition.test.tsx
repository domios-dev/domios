import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Terminal } from "@xterm/xterm";
import { __setInvokeForTest } from "../../lib/bridge";
import { useTerminalInput } from "./useTerminalInput";
import { useXterm } from "./useXterm";
import { setXtermFrameScheduler } from "./xtermInitScheduler";
import statusQueries from "../../../tests/fixtures/terminal-status.json";

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class { activate() {} dispose() {} fit() {} },
}));

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    clearRect() {}, fillRect() {},
  } as unknown as CanvasRenderingContext2D);
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  setXtermFrameScheduler(callback => callback());
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("xterm-char-measure-element") ? 8 * (this.textContent?.length ?? 32) : 640;
  });
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(16);
});

afterEach(() => {
  __setInvokeForTest(null);
  setXtermFrameScheduler(null);
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

async function fixture() {
  const host = document.createElement("div");
  const reactHost = document.createElement("div");
  document.body.append(host, reactHost);
  const writes: Array<Record<string, unknown> | undefined> = [];
  const data: string[] = [];
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    if (command === "write_pty_string") writes.push(args);
    return undefined as T;
  });
  let terminal: Terminal | null = null;
  function Harness() {
    const containerRef = useRef(host);
    const connectedPtyRef = useRef("composition-pty");
    const { termRef, termReady } = useXterm({ containerRef, renderEnabled: false });
    terminal = termRef.current;
    useTerminalInput({
      tabId: "composition-tab", termRef, connectedPtyRef, containerRef, termReady,
      terminalActive: true, onTabKey: () => false, onNonTabKey() {},
      onInputData: chunk => data.push(chunk),
    });
    return null;
  }
  const root = createRoot(reactHost);
  await act(async () => root.render(<Harness />));
  const term = terminal as Terminal | null;
  if (!term?.textarea) throw new Error("Terminal helper did not initialize");
  const textarea = term.textarea;
  return {
    term, textarea, data, writes,
    async dispose() {
      await act(async () => root.unmount());
      host.remove();
      reactHost.remove();
    },
  };
}

function enter(textarea: HTMLTextAreaElement, keyCode = 13, isComposing = false) {
  textarea.dispatchEvent(new KeyboardEvent("keydown", {
    key: "Enter", code: "Enter", keyCode, which: keyCode, isComposing,
    bubbles: true, cancelable: true,
  }));
  textarea.dispatchEvent(new KeyboardEvent("keyup", {
    key: "Enter", code: "Enter", keyCode, which: keyCode, bubbles: true,
  }));
}

describe("xterm composition delivery", () => {
  it("assigns DSR status to the backend with two real renderers and preserves CPR and DA", async () => {
    const control = new Terminal({ allowProposedApi: true });
    const controlReplies: string[] = [];
    control.onData(data => controlReplies.push(data));
    const first = await fixture();
    const second = await fixture();
    const firstReplies: string[] = [];
    const secondReplies: string[] = [];
    first.term.onData(data => firstReplies.push(data));
    second.term.onData(data => secondReplies.push(data));
    async function output(term: Terminal, chunks: Uint8Array[]) {
      const done = chunks.map(chunk => new Promise<void>(resolve => term.write(chunk, resolve)));
      await vi.runOnlyPendingTimersAsync();
      await Promise.all(done);
    }
    try {
      for (const query of statusQueries) {
        const bytes = Uint8Array.from(query.bytes);
        const splits = Array.from({ length: bytes.length + 1 }, (_, index) => [bytes.slice(0, index), bytes.slice(index)]);
        splits.push(Array.from(bytes, byte => Uint8Array.of(byte)));
        for (const chunks of splits) {
          control.reset(); first.term.reset(); second.term.reset();
          controlReplies.length = 0; firstReplies.length = 0; secondReplies.length = 0;
          await act(async () => {
            await output(control, chunks);
            await output(first.term, chunks);
            await output(second.term, chunks);
          });
          expect(controlReplies.filter(reply => reply === "\x1b[0n"), query.name)
            .toEqual(Array(query.statusReplies).fill("\x1b[0n"));
          expect(firstReplies, query.name).toEqual([]);
          expect(secondReplies, query.name).toEqual([]);
        }
      }
      await act(async () => {
        for (const term of [control, first.term, second.term]) {
          term.reset();
          await output(term, [new TextEncoder().encode("\x1b[6n\x1b[c")]);
        }
      });
      expect(firstReplies).toEqual(["\x1b[1;1R", "\x1b[?1;2c"]);
      expect(secondReplies).toEqual(["\x1b[1;1R", "\x1b[?1;2c"]);
    } finally {
      control.dispose();
      await first.dispose();
      await second.dispose();
    }
  });

  it("consumes renderer status queries without producing PTY input or changing user text", async () => {
    async function output(term: Terminal, ...chunks: string[]) {
      const parsed = chunks.map(chunk => new Promise<void>(resolve => term.write(chunk, resolve)));
      await vi.runOnlyPendingTimersAsync();
      await Promise.all(parsed);
    }
    const control = new Terminal({ allowProposedApi: true });
    const replies: string[] = [];
    control.onData(data => replies.push(data));
    try {
      await output(control, "\x1bP$qm\x1b\\");
      expect(replies).toEqual(["\x1bP1$r0m\x1b\\"]);
    } finally { control.dispose(); }

    const f = await fixture();
    const rawInput: string[] = [];
    f.term.onData(data => rawInput.push(data));
    try {
      const left = "f7a2z8";
      const right = "c9d4v1";
      for (const request of ["m", "r", " q", '"q', '"p', "k6w3"]) {
        const query = `\x1bP$q${request}\x1b\\`;
        for (let split = 0; split <= query.length; split += 1) {
          await act(async () => {
            await output(f.term, `\r\n${left}`, query.slice(0, split), query.slice(split), right);
          });
          const buffer = f.term.buffer.active;
          expect(buffer.getLine(buffer.baseY + buffer.cursorY)?.translateToString(true)).toBe(left + right);
        }
      }
      expect(rawInput).toEqual([]);
      expect(f.data).toEqual([]);
      expect(f.writes).toEqual([]);
      const userText = "q8n5r2m\\";
      await act(async () => f.term.paste(userText));
      expect(rawInput).toEqual([userText]);
      expect(f.data).toEqual([userText]);
      expect(f.writes).toEqual([{ ptyId: "composition-pty", data: userText }]);
    } finally { await f.dispose(); }
  });

  it("disables inline predictions before the helper receives focus", async () => {
    const focused: Array<string | null> = [];
    const focus = HTMLTextAreaElement.prototype.focus;
    vi.spyOn(HTMLTextAreaElement.prototype, "focus").mockImplementation(function (this: HTMLTextAreaElement, options) {
      focused.push(this.getAttribute("writingsuggestions"));
      focus.call(this, options);
    });
    const f = await fixture();
    try {
      expect(focused.length).toBeGreaterThan(0);
      expect(focused.every(value => value === "false")).toBe(true);
      expect(f.textarea.getAttribute("writingsuggestions")).toBe("false");
    } finally { await f.dispose(); }
  });

  it("writes committed IME text and the following Enter exactly once", async () => {
    const f = await fixture();
    try {
      await act(async () => {
        f.textarea.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
        f.textarea.value = "確定";
        f.textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "確定", bubbles: true }));
        await vi.advanceTimersByTimeAsync(0);
        enter(f.textarea, 229, true);
      });
      expect(f.writes).toEqual([]);
      await act(async () => {
        f.textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "確定", bubbles: true }));
        enter(f.textarea);
        await vi.runOnlyPendingTimersAsync();
      });
      expect(f.data).toEqual(["確定", "\r"]);
      expect(f.writes).toEqual([
        { ptyId: "composition-pty", data: "確定" },
        { ptyId: "composition-pty", data: "\r" },
      ]);
      expect(f.textarea.getAttribute("writingsuggestions")).toBe("false");
    } finally { await f.dispose(); }
  });

  it("writes plain Enter exactly once without composition", async () => {
    const f = await fixture();
    try {
      await act(async () => {
        enter(f.textarea);
        await vi.runOnlyPendingTimersAsync();
      });
      expect(f.data).toEqual(["\r"]);
      expect(f.writes).toEqual([{ ptyId: "composition-pty", data: "\r" }]);
      expect(f.textarea.getAttribute("writingsuggestions")).toBe("false");
    } finally { await f.dispose(); }
  });
});
