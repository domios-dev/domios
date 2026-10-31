import assert from "node:assert/strict";
import { test } from "node:test";
import { Window } from "happy-dom";
import { retainSidebarScrollAnchor } from "./sidebarScrollAnchor";

test("DOM anchoring retains a clicked header through maximum-scroll collapse, growth and scroll-up", () => {
  const fixture = scrollFixture();
  fixture.press(fixture.header);
  const before = fixture.header.getBoundingClientRect().top;
  fixture.resizeContent(1011);
  assert.equal(fixture.viewport.scrollTop, 427);
  assert.equal(fixture.header.getBoundingClientRect().top, before);
  assert.equal(fixture.space.offsetHeight, 132);

  fixture.scrollTo(377);
  assert.equal(fixture.space.offsetHeight, 82);
  fixture.resizeContent(1143);
  assert.equal(fixture.viewport.scrollTop, 377);
  assert.equal(fixture.space.offsetHeight, 0);

  fixture.resizeContent(1011);
  fixture.scrollTo(295);
  assert.equal(fixture.space.offsetHeight, 0);
  assert.equal(fixture.viewport.scrollTop, 295);
  fixture.dispose();
});

test("DOM anchoring compensates insertions above the first visible row without a press", () => {
  const fixture = scrollFixture();
  const before = fixture.header.getBoundingClientRect().top;
  fixture.moveHeader(60);
  assert.equal(fixture.viewport.scrollTop, 487);
  assert.equal(fixture.header.getBoundingClientRect().top, before);
  fixture.moveHeader(-60);
  assert.equal(fixture.viewport.scrollTop, 427);
  assert.equal(fixture.space.offsetHeight, 0);
  fixture.dispose();
});

test("DOM anchoring normalizes CSS zoom and releases space when the anchor is removed", () => {
  const fixture = scrollFixture(1.25);
  const before = fixture.header.getBoundingClientRect().top;
  fixture.resizeContent(1011);
  assert.equal(fixture.header.getBoundingClientRect().top, before);
  assert.equal(fixture.space.offsetHeight, 132);
  fixture.removeHeader();
  assert.equal(fixture.viewport.scrollTop, 295);
  assert.equal(fixture.space.offsetHeight, 0);
  fixture.dispose();
});

test("a tab switch that shrinks the content below the viewport settles at the top without an unreachable write", () => {
  const fixture = scrollFixture();
  const writes = fixture.writes();
  fixture.resizeContent(300);
  assert.equal(fixture.viewport.scrollTop, 0);
  assert.equal(fixture.space.offsetHeight, 0);
  assert.deepEqual(writes, []);
  fixture.dispose();
});

test("the pressed control takes priority over an earlier visible row", () => {
  const fixture = scrollFixture();
  fixture.prependAnchor();
  fixture.press(fixture.header);
  const before = fixture.header.getBoundingClientRect().top;
  fixture.moveHeader(60);
  assert.equal(fixture.header.getBoundingClientRect().top, before);
  fixture.dispose();
});

test("sortable drag transforms do not become scroll corrections", () => {
  const fixture = scrollFixture();
  fixture.setDragging(true);
  fixture.moveHeader(60);
  assert.equal(fixture.viewport.scrollTop, 427);
  fixture.setDragging(false);
  assert.equal(fixture.viewport.scrollTop, 427);
  fixture.moveHeader(20);
  assert.equal(fixture.viewport.scrollTop, 447);
  fixture.dispose();
});

test("pressing a sticky disclosure preserves that control rather than the content it reveals", () => {
  const fixture = scrollFixture();
  fixture.prependAnchor();
  fixture.header.style.position = "sticky";
  fixture.press(fixture.header);
  const before = fixture.header.getBoundingClientRect().top;
  fixture.moveEarlier(60);
  assert.equal(fixture.viewport.scrollTop, 427);
  assert.equal(fixture.header.getBoundingClientRect().top, before);
  fixture.dispose();
});

function scrollFixture(zoom = 1) {
  const window = new Window();
  const document = window.document;
  const callbacks: { mutation?: () => void; resize?: () => void } = {};
  const observed = new Set<object>();
  class MutationObserverFixture {
    constructor(callback: () => void) { callbacks.mutation = callback; }
    observe(target: object) { observed.add(target); }
    disconnect() { callbacks.mutation = undefined; }
  }
  class ResizeObserverFixture {
    constructor(callback: () => void) { callbacks.resize = callback; }
    observe(target: object) { observed.add(target); }
    disconnect() { callbacks.resize = undefined; }
  }
  Object.assign(globalThis, {
    Element: window.Element,
    getComputedStyle: window.getComputedStyle.bind(window),
    MutationObserver: MutationObserverFixture,
    ResizeObserver: ResizeObserverFixture,
  });
  const viewport = document.createElement("div");
  const body = document.createElement("div");
  const space = document.createElement("div");
  const header = document.createElement("button");
  header.className = "sidebar-team-workers-toggle";
  header.textContent = "Workers";
  body.append(header);
  viewport.append(body, space);
  document.body.append(viewport);
  let contentHeight = 1143;
  let scrollTop = 427;
  let headerTop = 934;
  let earlierTop = 500;
  const viewportHeight = 716;
  const writes: number[] = [];
  let observing = false;
  const notify = (callback?: () => void) => {
    observing = true;
    try { callback?.(); } finally { observing = false; }
  };
  const box = (top: number, height: number) => new window.DOMRect(0, top * zoom, 280 * zoom, height * zoom);
  Object.defineProperties(space, { offsetHeight: { get: () => Number.parseFloat(space.style.height) || 0 } });
  Object.defineProperties(viewport, {
    clientHeight: { get: () => viewportHeight },
    offsetHeight: { get: () => viewportHeight },
    scrollHeight: { get: () => Math.max(viewportHeight, contentHeight + space.offsetHeight) },
    scrollTop: {
      get: () => scrollTop,
      set: (next: number) => {
        if (observing) writes.push(next);
        scrollTop = Math.max(0, Math.min(next, viewport.scrollHeight - viewportHeight));
      },
    },
  });
  viewport.getBoundingClientRect = () => box(0, viewportHeight);
  header.getBoundingClientRect = () => box(headerTop - scrollTop, 27);
  header.getClientRects = () => Object.assign([header.getBoundingClientRect()], { item: () => header.getBoundingClientRect() });
  const dispose = retainSidebarScrollAnchor(viewport as unknown as HTMLElement, body as unknown as HTMLElement, space as unknown as HTMLElement);
  assert.ok(observed.has(body) && observed.has(viewport), "body mutations and both resize surfaces are observed");
  return {
    viewport, space, header,
    writes: () => writes,
    press: (element: typeof header) => element.dispatchEvent(new window.Event("pointerdown", { bubbles: true })),
    resizeContent: (height: number) => {
      contentHeight = height;
      viewport.scrollTop = scrollTop;
      notify(callbacks.mutation);
      notify(callbacks.resize);
    },
    moveHeader: (delta: number) => {
      headerTop += delta;
      contentHeight += delta;
      callbacks.mutation?.();
    },
    moveEarlier: (delta: number) => {
      earlierTop += delta;
      contentHeight += delta;
      callbacks.mutation?.();
    },
    removeHeader: () => { header.remove(); callbacks.mutation?.(); },
    setDragging: (dragging: boolean) => {
      header.setAttribute("data-dnd-dragging", String(dragging));
      callbacks.mutation?.();
    },
    prependAnchor: () => {
      const earlier = document.createElement("div");
      earlier.className = "sidebar-tab";
      earlier.getBoundingClientRect = () => box(earlierTop - scrollTop, 44);
      earlier.getClientRects = () => Object.assign([earlier.getBoundingClientRect()], { item: () => earlier.getBoundingClientRect() });
      body.prepend(earlier);
      callbacks.mutation?.();
    },
    scrollTo: (next: number) => {
      viewport.scrollTop = next;
      viewport.dispatchEvent(new window.Event("scroll"));
    },
    dispose: () => {
      dispose();
      assert.equal(callbacks.mutation, undefined);
      assert.equal(callbacks.resize, undefined);
      viewport.remove();
    },
  };
}
