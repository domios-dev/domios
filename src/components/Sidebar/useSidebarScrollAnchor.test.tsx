import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { useSidebarScrollAnchor } from "./useSidebarScrollAnchor";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Probe({ active, selected }: { active: string | null; selected: string | null }) {
  const ref = useSidebarScrollAnchor(active, selected);
  return <div ref={ref} data-testid="scroller" />;
}

let root: Root;
let host: HTMLElement;
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });

async function mount() {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<Probe active="a" selected="a" />));
  const scroller = host.querySelector<HTMLElement>('[data-testid="scroller"]')!;
  const access = { top: 0, reads: 0, writes: 0 };
  Object.defineProperty(scroller, "scrollTop", {
    get() { access.reads++; return access.top; },
    set(value: number) { access.writes++; access.top = value; },
  });
  return { scroller, access };
}

it("switches without touching scrollTop while the list is already at the top", async () => {
  const { access } = await mount();
  await act(async () => root.render(<Probe active="b" selected="b" />));
  expect(access).toEqual({ top: 0, reads: 0, writes: 0 });
});

it("returns a scrolled list to the top on a switch and leaves it alone otherwise", async () => {
  const { scroller, access } = await mount();
  access.top = 240;
  await act(async () => { scroller.dispatchEvent(new Event("scroll")); });
  await act(async () => root.render(<Probe active="a" selected="a" />));
  expect(access.top).toBe(240);
  access.reads = 0;
  await act(async () => root.render(<Probe active="b" selected="a" />));
  expect(access.top).toBe(0);
  expect(access.writes).toBe(1);
  expect(access.reads).toBe(0);
});
