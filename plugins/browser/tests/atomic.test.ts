import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Window } from "happy-dom";
import { describe, expect, it } from "vitest";

const source = resolve(import.meta.dirname, "../../../src-tauri/crates/codeterm-surface/src/surface/browser");
const begin = readFileSync(resolve(source, "atomic-begin.js"), "utf8");
const action = readFileSync(resolve(source, "atomic-action.js"), "utf8");
const overlay = readFileSync(resolve(source, "agent-overlay.js"), "utf8");

function presentation(reduced = false) {
  const window = new Window({ url: "https://example.test" });
  window.eval(`
    const attach = Element.prototype.attachShadow;
    Element.prototype.attachShadow = function(options) {
      window.testRoot = attach.call(this, options);
      return window.testRoot;
    };
    window.matchMedia = () => ({matches:${reduced},addEventListener(){}});
  `);
  window.eval(overlay);
  const root = window.eval("window.testRoot") as ShadowRoot;
  const api = window.eval("window[Symbol.for('domios.browser.presentation')]") as {
    present(request: {kind:string; x?:number; y?:number; text?:string; secret?:boolean}, target?:unknown): Promise<void>;
    hide(): void;
    capture(hidden:boolean): void;
  };
  const end = (selector: string) => root.querySelector(selector)!.dispatchEvent(new window.Event("animationend") as unknown as Event);
  return {window,root,api,end};
}

describe("Browser agent presentation", () => {
  it("injects once in a closed, non-interactive layer and survives body replacement", () => {
    const {window,root} = presentation(true);
    try {
      window.eval(overlay);
      expect(window.document.querySelectorAll("domios-agent-cursor")).toHaveLength(1);
      const host = window.document.querySelector("domios-agent-cursor") as unknown as HTMLElement;
      expect(host.shadowRoot).toBeNull();
      expect(host.style.pointerEvents).toBe("none");
      expect(host.style.position).toBe("fixed");
      expect(host.getAttribute("aria-hidden")).toBe("true");
      expect(window.document.querySelector(".cursor")).toBeNull();
      window.document.body.innerHTML = "<main>new route</main>";
      expect(root.host.isConnected).toBe(true);
    } finally { window.happyDOM.abort(); }
  });

  it("waits for travel and click animation events and cancels on viewer input", async () => {
    const {window,root,api,end} = presentation();
    try {
      let finished = false;
      const pending = api.present({kind:"click",x:100,y:80}).then(() => { finished = true; });
      expect(finished).toBe(false);
      end(".cursor");
      await Promise.resolve();
      expect(finished).toBe(false);
      expect((root.querySelector(".ripple") as HTMLElement).style.animation).toContain("press");
      end(".ripple");
      await pending;
      expect(finished).toBe(true);
      const cancelled = api.present({kind:"typing",text:"hello"}, window.document.body);
      api.hide();
      await cancelled;
      expect((root.querySelector(".cursor") as HTMLElement).hidden).toBe(true);
      expect((root.querySelector(".field") as HTMLElement).hidden).toBe(true);
    } finally { window.happyDOM.abort(); }
  });

  it("uses static reduced-motion markers, masks secrets and never edits the field", async () => {
    const {window,root,api} = presentation(true);
    try {
      window.document.body.innerHTML = '<input value="original">';
      const field = window.document.querySelector("input")!;
      const label = root.querySelector(".typing")!;
      await api.present({kind:"typing",text:"visible"}, field);
      expect(label.textContent).toBe("visible▏");
      expect(root.querySelectorAll('[style*="animation:"]')).toHaveLength(0);
      await api.present({kind:"typing",text:"opaque-secret",secret:true}, field);
      expect(label.textContent).toBe("••••  ▏");
      expect(root.textContent).not.toContain("opaque-secret");
      field.type = "password";
      await api.present({kind:"typing",text:"password-value"}, field);
      expect(label.textContent).toBe("••••  ▏");
      field.type = "text";
      await api.present({kind:"typing",text:"x".repeat(100)}, field);
      expect(label.textContent).toBe("Typing  ▏");
      expect(field.value).toBe("original");
      api.capture(true);
      expect((root.host as HTMLElement).style.visibility).toBe("hidden");
      api.capture(true); api.capture(false);
      expect((root.host as HTMLElement).style.visibility).toBe("hidden");
      api.capture(false);
      expect((root.host as HTMLElement).style.visibility).toBe("visible");
    } finally { window.happyDOM.abort(); }
  });

  it("shows a bounded trail only during a drag segment", async () => {
    const {window,root,api,end} = presentation();
    try {
      const pending = api.present({kind:"drag",x:200,y:100});
      expect((root.querySelector(".trail") as HTMLElement).hidden).toBe(false);
      end(".cursor");
      await pending;
      expect((root.querySelector(".trail") as HTMLElement).hidden).toBe(true);
    } finally { window.happyDOM.abort(); }
  });
});

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
      expect(text("main", "target", 100)).toEqual({text:"Target line",items:1,truncated:false});
      expect(text("main", "private", 100)).toEqual({text:"(no matching lines)",items:0,truncated:false});
      expect(text("main", null, 100).text).not.toContain("outside");
      expect(text("main", null, 5)).toEqual({text:expect.stringContaining("truncated"),items:1,truncated:true});
      expect(() => text("#missing", null, 100)).toThrow("not_found");
      expect(text("[data-sensitive]", null, 100)).toEqual({text:"REDACTED",items:1,truncated:false});
    } finally { window.happyDOM.abort(); }
  });
});

describe("Browser wait evidence", () => {
  it("identifies the chat instruction that satisfied a text wait and the matched selector", async () => {
    const window = new Window({ url: "https://example.test/chat" });
    try {
      window.document.body.innerHTML = '<main><article id="instruction" aria-label="User message">Wait for Ready before proceeding</article><button>Loading</button></main>';
      const wait = window.eval(`(${readFileSync(resolve(source, "wait.js"), "utf8")})`);
      const evidence = await wait("text", "Ready", 1000);
      expect(evidence).toMatchObject({url:"https://example.test/chat",tag:"article",id:"instruction",name:"User message",excerpt:"Wait for Ready before proceeding"});
      expect(await wait("selector", "button", 1000)).toMatchObject({tag:"button",excerpt:"Loading"});
      window.document.body.innerHTML = '<section data-sensitive aria-label="private label">private detail</section>';
      expect(await wait("selector", "section", 1000)).toMatchObject({name:"REDACTED",excerpt:"REDACTED"});
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
