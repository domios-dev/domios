import assert from "node:assert/strict";
import test from "node:test";
import { act, useRef, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { AnchoredPopover } from "./AnchoredPopover";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const window = new Window({ url: "http://localhost/anchored-popover" });
for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "KeyboardEvent", "PointerEvent", "MouseEvent", "MutationObserver"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? window : (window as unknown as Record<string, unknown>)[key], configurable: true });
}

function Harness({ closes, nested, children }: { closes: string[]; nested?: boolean; children?: ReactNode }) {
  const trigger = useRef<HTMLButtonElement>(null);
  const inner = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(true);
  const [innerOpen, setInnerOpen] = useState(Boolean(nested));
  return (
    <div onKeyDown={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()}>
      <button ref={trigger} type="button" data-testid="trigger">Open</button>
      <button type="button" data-testid="elsewhere">Elsewhere</button>
      <AnchoredPopover open={open} anchor={trigger} onClose={() => { closes.push("outer"); setOpen(false); }}>
        <div data-testid="outer">
          <button ref={inner} type="button" data-testid="inner-trigger">More</button>
          {children}
          <AnchoredPopover open={innerOpen} anchor={inner} onClose={() => { closes.push("inner"); setInnerOpen(false); }}>
            <button type="button" data-testid="inner-item">Item</button>
          </AnchoredPopover>
        </div>
      </AnchoredPopover>
    </div>
  );
}

async function mount(ui: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  const by = (id: string) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
  return {
    by,
    escape: (id: string) => act(async () => { by(id)!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true })); }),
    press: (id: string) => act(async () => { by(id)!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); }),
    unmount: async () => { await act(async () => root.unmount()); host.remove(); },
  };
}

test("Escape and outside presses close even when the trigger's container stops propagation", async () => {
  const closes: string[] = [];
  const view = await mount(<Harness closes={closes} />);
  await view.press("elsewhere");
  assert.deepEqual(closes, ["outer"]);
  assert.equal(view.by("outer"), null);
  await view.unmount();

  const escapes: string[] = [];
  const escaped = await mount(<Harness closes={escapes} />);
  escaped.by("trigger")!.focus();
  await escaped.escape("trigger");
  assert.deepEqual(escapes, ["outer"]);
  assert.equal(document.activeElement, escaped.by("trigger"));
  await escaped.unmount();
});

test("a press or Escape inside a nested popover closes only that popover", async () => {
  const closes: string[] = [];
  const view = await mount(<Harness closes={closes} nested />);
  await view.press("inner-item");
  assert.deepEqual(closes, [], "pressing inside the nested popover keeps both open");
  await view.escape("inner-item");
  assert.deepEqual(closes, ["inner"], "Escape inside the nested popover closes it alone");
  assert.ok(view.by("outer"));
  await view.escape("inner-trigger");
  assert.deepEqual(closes, ["inner", "outer"]);
  await view.unmount();
});

test("an Escape the popover's content already handled is left alone", async () => {
  const closes: string[] = [];
  const view = await mount(
    <Harness closes={closes}>
      <input data-testid="field" onKeyDown={(event) => { if (event.key === "Escape") event.preventDefault(); }} />
    </Harness>,
  );
  await view.escape("field");
  assert.deepEqual(closes, []);
  await view.unmount();
});

const OPTION_HEIGHT = 40;
const CHROME_HEIGHT = 24;

function GrowingList({ count }: { count: number }) {
  const trigger = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button ref={trigger} type="button">Mention</button>
      <AnchoredPopover open anchor={trigger}>
        <div style={{ maxHeight: "inherit" }}>
          <div role="listbox" aria-label="Suggestions">
            {Array.from({ length: count }, (_, index) => <div key={index} role="option" aria-selected={false}>{index}</div>)}
          </div>
        </div>
      </AnchoredPopover>
    </>
  );
}

test("the popover grows to fit options that arrive after it was first measured", async () => {
  const offsetHeight = Object.getOwnPropertyDescriptor(window.HTMLElement.prototype, "offsetHeight");
  Object.defineProperty(window.HTMLElement.prototype, "offsetHeight", { configurable: true, get(this: HTMLElement) {
    return this.hasAttribute("data-anchored-popover") ? CHROME_HEIGHT + this.querySelectorAll('[role="option"]').length * OPTION_HEIGHT : 0;
  } });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<GrowingList count={0} />));
    const popover = () => document.querySelector<HTMLElement>("[data-anchored-popover]")!;
    assert.equal(popover().style.maxHeight, `${CHROME_HEIGHT}px`);
    await act(async () => root.render(<GrowingList count={8} />));
    assert.equal(popover().style.maxHeight, `${CHROME_HEIGHT + 8 * OPTION_HEIGHT}px`);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    if (offsetHeight) Object.defineProperty(window.HTMLElement.prototype, "offsetHeight", offsetHeight);
  }
});
