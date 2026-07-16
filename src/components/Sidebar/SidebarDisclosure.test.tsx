import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { SidebarDisclosure } from "./SidebarDisclosure";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function render(expanded: boolean): Promise<{ button: HTMLButtonElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<SidebarDisclosure expanded={expanded} aria-label="Toggle">Label</SidebarDisclosure>);
  });
  const button = host.querySelector("button");
  assert(button instanceof HTMLButtonElement, "disclosure is a native button");
  return { button, root };
}

const window = new Window({ url: "http://localhost/" });
Object.assign(globalThis, {
  window,
  document: window.document,
  HTMLElement: window.HTMLElement,
  HTMLButtonElement: window.HTMLButtonElement,
  SVGElement: window.SVGElement,
});
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function run() {
  const closed = await render(false);
  assert(closed.button.getAttribute("aria-expanded") === "false", "closed disclosure exposes aria-expanded=false");
  assert(closed.button.querySelector(".sidebar-disclosure-glyph"), "shared glyph renders once");
  assert(!closed.button.hasAttribute("onkeydown"), "native button keeps Enter and Space activation");
  await act(async () => closed.root.unmount());

  const open = await render(true);
  assert(open.button.getAttribute("aria-expanded") === "true", "open disclosure exposes aria-expanded=true");
  const css = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");
  for (const rule of ["--sidebar-disclosure-size", "--sidebar-disclosure-stroke-width", "--sidebar-disclosure-color", "--sidebar-disclosure-transition", "rotate(90deg)"]) {
    assert(css.includes(rule), `shared CSS owns ${rule}`);
  }
  await act(async () => open.root.unmount());
  console.log("SidebarDisclosure tests: passed");
}

run().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
