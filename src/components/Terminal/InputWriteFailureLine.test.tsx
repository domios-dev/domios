import "./ChatComposer.composition.harness";
import { readFileSync } from "node:fs";
import { act } from "react";
import ts from "typescript";
import { createRoot } from "react-dom/client";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import { useInputWriteFailure } from "../../hooks/terminal/useInputWriteFailure";
import { TERMINAL_INPUT_WRITE_FAILED_EVENT, TERMINAL_INPUT_WRITE_SUCCEEDED_EVENT } from "../../hooks/terminal/terminalInputWrite";
import { InputWriteFailureLine } from "./InputWriteFailureLine";

describe("inline terminal input status wiring", () => {
  it("places the event status above the input, dismisses once and clears on matching success", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    let dismissals = 0;
    function Harness() {
      const failure = useInputWriteFailure("mesh:a/tab", "pty");
      return <>{failure.visible && <InputWriteFailureLine onDismiss={() => { dismissals++; failure.dismiss(); }} />}<textarea aria-label="Composer" /></>;
    }
    const emit = async (event: string, targetId: string) => {
      await act(async () => { window.dispatchEvent(new CustomEvent(event, { detail: { targetId } })); });
    };
    try {
      await act(async () => root.render(<Harness />));
      await emit(TERMINAL_INPUT_WRITE_FAILED_EVENT, "mesh:b/tab");
      expect(host.querySelector('[role="status"]')).toBe(null);
      await emit(TERMINAL_INPUT_WRITE_FAILED_EVENT, "mesh:a/tab");
      const status = host.querySelector('[role="status"]')!;
      expect(status.nextElementSibling).toBe(host.querySelector("textarea"));
      expect(Boolean(status.getAttribute("aria-label"))).toBe(true);
      await act(async () => { host.querySelector("button")!.click(); });
      expect(dismissals).toBe(1);
      expect(host.querySelector('[role="status"]')).toBe(null);
      await emit(TERMINAL_INPUT_WRITE_FAILED_EVENT, "mesh:a/tab");
      await emit(TERMINAL_INPUT_WRITE_SUCCEEDED_EVENT, "mesh:b/tab");
      expect(Boolean(host.querySelector('[role="status"]'))).toBe(true);
      await emit(TERMINAL_INPUT_WRITE_SUCCEEDED_EVENT, "mesh:a/tab");
      expect(host.querySelector('[role="status"]')).toBe(null);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
  it("connects the shared status to both modes without the notification store", () => {
    const terminal = readFileSync(new URL("./TerminalTab.tsx", import.meta.url), "utf8");
    const chat = readFileSync(new URL("./TabSessionChat.tsx", import.meta.url), "utf8");
    expect(terminal.includes("addNotification")).toBe(false);
    expect(terminal.includes("inputStatus={inputFailureLine}")).toBe(true);
    expect(terminal.includes('uiMode === "terminal" && !showTerminalKeyBar && inputFailureLine')).toBe(true);
    const source = ts.createSourceFile("TabSessionChat.tsx", chat, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    let composerPosition = -1;
    const visit = (node: ts.Node) => {
      if ((ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) && node.tagName.getText(source) === "ChatComposer") composerPosition = node.getStart(source);
      ts.forEachChild(node, visit);
    };
    visit(source);
    expect(composerPosition >= 0).toBe(true);
    expect(chat.indexOf("{inputStatus &&") >= 0 && chat.indexOf("{inputStatus &&") < composerPosition).toBe(true);
  });
});
finish();
