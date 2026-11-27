import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { useSidebarScrollAnchor } from "./useSidebarScrollAnchor";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Probe({ selected, cardBounds }: { selected: string | null; cardBounds: Record<string, { top: number; bottom: number }> }) {
  const ref = useSidebarScrollAnchor(selected);
  return <div ref={ref} data-testid="scroller">
    {Object.keys(cardBounds).map((id) => (
      <div key={id} data-testid="workspace-card" data-workspace-id={id} data-selected={id === selected ? "true" : "false"} />
    ))}
  </div>;
}

let root: Root;
let host: HTMLElement;
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });

function stubRects(scroller: HTMLElement, viewport: { top: number; bottom: number }, cards: Record<string, { top: number; bottom: number }>) {
  scroller.getBoundingClientRect = () => new DOMRect(0, viewport.top, 260, viewport.bottom - viewport.top);
  for (const [id, bounds] of Object.entries(cards)) {
    const card = scroller.querySelector<HTMLElement>(`[data-workspace-id="${id}"]`);
    if (card) card.getBoundingClientRect = () => new DOMRect(0, bounds.top, 42, bounds.bottom - bounds.top);
  }
}

async function mount(cardBounds: Record<string, { top: number; bottom: number }>, initialTop = 240) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<Probe selected="a" cardBounds={cardBounds} />));
  const scroller = host.querySelector<HTMLElement>('[data-testid="scroller"]')!;
  const access = { top: initialTop, reads: 0, writes: 0 };
  Object.defineProperty(scroller, "scrollTop", {
    get() { access.reads++; return access.top; },
    set(value: number) { access.writes++; access.top = value; },
  });
  return { scroller, access };
}

it("keeps scrollTop untouched when switching to a visible workspace card", async () => {
  const bounds = { a: { top: 120, bottom: 160 }, b: { top: 200, bottom: 240 } };
  const { scroller, access } = await mount(bounds, 240);
  stubRects(scroller, { top: 100, bottom: 500 }, bounds);
  await act(async () => root.render(<Probe selected="b" cardBounds={bounds} />));
  expect(access).toEqual({ top: 240, reads: 0, writes: 0 });
});

it("scrolls minimally when the newly active workspace card is off-screen above or below", async () => {
  const bounds = { a: { top: 120, bottom: 160 }, above: { top: 40, bottom: 80 }, below: { top: 530, bottom: 570 } };
  const { scroller, access } = await mount(bounds, 240);
  stubRects(scroller, { top: 100, bottom: 500 }, bounds);

  await act(async () => root.render(<Probe selected="above" cardBounds={bounds} />));
  expect(access.top).toBe(180);
  expect(access.writes).toBe(1);

  await act(async () => root.render(<Probe selected="below" cardBounds={bounds} />));
  expect(access.top).toBe(250);
  expect(access.writes).toBe(2);

  await act(async () => root.render(<Probe selected="below" cardBounds={bounds} />));
  expect(access.writes).toBe(2);
});
