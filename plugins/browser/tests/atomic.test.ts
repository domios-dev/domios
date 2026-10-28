import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Window } from "happy-dom";
import { describe, expect, it } from "vitest";

const source = resolve(import.meta.dirname, "../../../src-tauri/crates/codeterm-surface/src/surface/browser");
const begin = readFileSync(resolve(source, "atomic-begin.js"), "utf8");
const action = readFileSync(resolve(source, "atomic-action.js"), "utf8");

describe("Browser atomic page action", () => {
  it("refuses mutation after capture and dispatches a current action once", () => {
    const window = new Window({ url: "http://localhost:7686" });
    try {
      window.document.body.innerHTML = '<button>Save</button>';
      const button = window.document.querySelector("button")!;
      button.getClientRects = () => [{ width: 20 }] as unknown as ReturnType<typeof button.getClientRects>;
      let clicks = 0;
      button.addEventListener("click", () => clicks++);
      const epoch = window.eval(begin);
      button.setAttribute("aria-label", "Changed");
      const execute = window.eval(`(${action})`);
      expect(execute.call(button, epoch, "click", null)).toBe("stale");
      expect(clicks).toBe(0);
      const fresh = window.eval(begin);
      expect(execute.call(button, fresh, "click", null)).toBe("performed");
      expect(clicks).toBe(1);
      button.remove();
      expect(execute.call(button, fresh, "click", null)).toBe("stale");
      expect(clicks).toBe(1);
    } finally { window.happyDOM.abort(); }
  });
  it("fills through the native setter and refuses disabled fields", () => {
    const window = new Window({ url: "http://localhost:7686" });
    try {
      window.document.body.innerHTML = '<input value="before">';
      const input = window.document.querySelector("input")!;
      input.getClientRects = () => [{ width: 20 }] as unknown as ReturnType<typeof input.getClientRects>;
      let inputs = 0;
      input.addEventListener("input", () => inputs++);
      const execute = window.eval(`(${action})`);
      expect(execute.call(input, window.eval(begin), "fill", "after")).toBe("performed");
      expect(input.value).toBe("after");
      expect(inputs).toBe(1);
      input.disabled = true;
      expect(execute.call(input, window.eval(begin), "fill", "wrong")).toBe("disabled");
      expect(input.value).toBe("after");
    } finally { window.happyDOM.abort(); }
  });
});
