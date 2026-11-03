import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Window } from "happy-dom";
import { describe, expect, it } from "vitest";

const source = resolve(import.meta.dirname, "../../../src-tauri/crates/codeterm-surface/src/surface/browser");
const begin = readFileSync(resolve(source, "atomic-begin.js"), "utf8");
const action = readFileSync(resolve(source, "atomic-action.js"), "utf8");

describe("Browser visible typing and submission", () => {
  it("refuses hidden, disabled and unfocusable fields before clearing their values", () => {
    const window = new Window({ url: "https://example.test" });
    try {
      window.document.body.innerHTML = '<input value="keep"><input value="mobile" style="display:none">';
      const [input, hidden] = window.document.querySelectorAll("input");
      const prepare = window.eval(`(${readFileSync(resolve(source, "type-focus.js"), "utf8")})`);
      const visible = window.eval(`(${readFileSync(resolve(source, "visible.js"), "utf8")})`);
      expect(prepare.call(hidden, false)).toBe("hidden");
      expect(hidden.value).toBe("mobile");
      input.getClientRects = () => [{ width: 20 }] as unknown as ReturnType<typeof input.getClientRects>;
      expect(visible.call(input)).toBe(true);
      expect(visible.call(hidden)).toBe(false);
      input.disabled = true;
      expect(prepare.call(input, false)).toBe("disabled");
      expect(input.value).toBe("keep");
      input.disabled = false;
      input.style.visibility = "hidden";
      expect(prepare.call(input, false)).toBe("hidden");
      input.style.visibility = "visible";
      input.focus = () => {};
      expect(prepare.call(input, false)).toBe("focus_failed");
      expect(input.value).toBe("keep");
    } finally { window.happyDOM.abort(); }
  });

  it("masks and clears only the focused field and refuses focus lost during input", () => {
    const window = new Window({ url: "https://example.test" });
    try {
      window.document.body.innerHTML = '<input value="before"><button>Other</button>';
      const input = window.document.querySelector("input")!;
      input.getClientRects = () => [{ width: 20 }] as unknown as ReturnType<typeof input.getClientRects>;
      const prepare = window.eval(`(${readFileSync(resolve(source, "type-focus.js"), "utf8")})`);
      expect(prepare.call(input, true)).toBe("focused");
      expect(input.type).toBe("password");
      expect(input.value).toBe("");
      input.addEventListener("input", () => window.document.querySelector("button")!.focus());
      expect(prepare.call(input, false)).toBe("focus_lost");
    } finally { window.happyDOM.abort(); }
  });

  it("uses one requestSubmit fallback, accepts Enter submit evidence and cleans observers", () => {
    const window = new Window({ url: "https://example.test" });
    try {
      window.document.body.innerHTML = '<form><input></form>';
      const input = window.document.querySelector("input")!;
      const form = window.document.querySelector("form")!;
      input.focus();
      let submissions = 0;
      form.addEventListener("submit", event => { submissions++; event.preventDefault(); });
      const submit = window.eval(`(${readFileSync(resolve(source, "submit.js"), "utf8")})`);
      expect(submit.call(input, "begin", "probe")).toBe("ready");
      expect(submit.call(input, "finish", "probe")).toBe("submitted");
      expect(submissions).toBe(1);
      expect(submit.call(input, "finish", "probe")).toBe("submitted");
      expect(submissions).toBe(1);
      expect(submit.call(input, "cleanup", "probe")).toBe("cleaned");
      expect(submit.call(input, "finish", "probe")).toBe("missing");
      expect(submit.call(input, "begin", "probe")).toBe("ready");
      form.dispatchEvent(new window.Event("submit", { bubbles:true, cancelable:true }));
      expect(submit.call(input, "finish", "probe")).toBe("submitted");
      expect(submissions).toBe(2);
      submit.call(input, "cleanup", "probe");
    } finally { window.happyDOM.abort(); }
  });

  it("refuses fallback after focus changes and requires actual submit evidence", () => {
    const window = new Window({ url: "https://example.test" });
    try {
      window.document.body.innerHTML = '<form><input required><button>Other</button></form>';
      const input = window.document.querySelector("input")!;
      const submit = window.eval(`(${readFileSync(resolve(source, "submit.js"), "utf8")})`);
      input.focus();
      expect(submit.call(input, "begin", "probe")).toBe("ready");
      expect(submit.call(input, "finish", "probe")).toBe("submit_unconfirmed");
      window.document.querySelector("button")!.focus();
      expect(submit.call(input, "finish", "probe")).toBe("focus_lost");
      submit.call(input, "cleanup", "probe");
      input.removeAttribute("required");
      input.remove();
      window.document.body.append(input);
      input.focus();
      expect(submit.call(input, "begin", "probe")).toBe("ready");
      expect(submit.call(input, "finish", "probe")).toBe("no_form");
      submit.call(input, "cleanup", "probe");
    } finally { window.happyDOM.abort(); }
  });
});

describe("Browser filtered text", () => {
  it("filters before bounding, scopes content and redacts sensitive subtrees", () => {
    const window = new Window({ url: "https://example.test" });
    try {
      window.document.body.innerHTML = '<aside>outside</aside><main>intro<br>Target line<br><span data-sensitive>private detail</span></main>';
      Object.defineProperty(window.document.querySelector("main"), "innerText", { value:"intro\nTarget line\nprivate detail" });
      const text = window.eval(`(${readFileSync(resolve(source, "text.js"), "utf8")})`);
      expect(text("main", "target", 100)).toBe("Target line");
      expect(text("main", "private", 100)).toBe("(no matching lines)");
      expect(text("main", null, 100)).not.toContain("outside");
      expect(text("main", null, 5)).toContain("truncated");
      expect(() => text("#missing", null, 100)).toThrow("not_found");
      expect(text("[data-sensitive]", null, 100)).toBe("REDACTED");
    } finally { window.happyDOM.abort(); }
  });
});

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

describe("Browser form control actions", () => {
  it("sets checkboxes idempotently, selects by value or text and rejects invalid values", () => {
    const window = new Window({ url: "https://example.test" });
    try {
      window.document.body.innerHTML = '<input type="checkbox"><select><option value="a">First</option><option value="b">Second</option><option value="c" disabled>Third</option></select>';
      const input = window.document.querySelector("input")!;
      const select = window.document.querySelector("select")!;
      for (const element of [input, select]) element.getClientRects = () => [{ width: 20 }] as unknown as ReturnType<typeof element.getClientRects>;
      const execute = window.eval(`(${readFileSync(resolve(source, "element-action.js"), "utf8")})`);
      let clicks = 0;
      input.addEventListener("click", () => clicks++);
      const run = (element: unknown, value: string) => execute.call(element, window.eval(begin), "form", value);
      expect(run(input, "true")).toBe("performed");
      expect(input.checked).toBe(true);
      expect(run(input, "true")).toBe("performed");
      expect(clicks).toBe(1);
      expect(run(input, "false")).toBe("performed");
      expect(input.checked).toBe(false);
      expect(run(input, "yes")).toBe("invalid_checkbox_value");
      expect(run(select, "b")).toBe("performed");
      expect(select.value).toBe("b");
      expect(run(select, "First")).toBe("performed");
      expect(select.value).toBe("a");
      expect(run(select, "Third")).toBe("disabled");
      expect(run(select, "missing")).toBe("not_found");
      const epoch = window.eval(begin);
      select.setAttribute("data-changed", "true");
      expect(execute.call(select, epoch, "form", "b")).toBe("stale");
      expect(select.value).toBe("a");
    } finally { window.happyDOM.abort(); }
  });

  it("refuses a pointer prepared before mutation or user input", () => {
    const window = new Window({ url: "https://example.test" });
    try {
      const captureSource = readFileSync(resolve(source, "pointer-begin.js"), "utf8");
      const check = window.eval(readFileSync(resolve(source, "pointer-check.js"), "utf8"));
      const capture = window.eval(captureSource);
      expect(check(capture)).toBe(true);
      window.document.body.innerHTML = "changed";
      expect(check(capture)).toBe(false);
      const fresh = window.eval(captureSource);
      expect(check(fresh)).toBe(true);
      window.document.dispatchEvent(new window.KeyboardEvent("keydown"));
      expect(check(fresh)).toBe(false);
    } finally { window.happyDOM.abort(); }
  });
});
