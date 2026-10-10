import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { createKeyedFlagStore } from "./keyedFlagStore";
import { surfaceControlStore, useSurfaceControlled, useSurfaceTakeControl } from "./surfaceControlStore";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLElement | null = null;

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
  surfaceControlStore.reset();
});

async function mount(ui: React.ReactElement) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(ui));
}

let toggle: (() => void) | null = null;

function View({ id }: { id?: string }) {
  const [on, setOn] = useSurfaceTakeControl(id);
  toggle = () => setOn((value) => !value);
  return <span data-testid="view">{on ? "on" : "off"}</span>;
}

function Reader({ id }: { id: string }) {
  return <span data-testid="reader">{useSurfaceControlled(id) ? "on" : "off"}</span>;
}

const text = (id: string) => host!.querySelector(`[data-testid='${id}']`)?.textContent;

describe("keyed flag store", () => {
  it("notifies only on real changes", () => {
    const store = createKeyedFlagStore();
    let calls = 0;
    store.subscribe(() => { calls += 1; });
    store.set("a", true);
    store.set("a", true);
    store.set("b", false);
    expect(store.get("a")).toBe(true);
    expect(calls).toBe(1);
  });
});

describe("take control store", () => {
  it("shares a keyed surface's control with other readers", async () => {
    await mount(<><View id="s1" /><Reader id="s1" /><Reader id="s2" /></>);
    expect(text("reader")).toBe("off");
    await act(async () => toggle!());
    expect(text("view")).toBe("on");
    expect(surfaceControlStore.get("s1")).toBe(true);
    expect(host!.querySelectorAll("[data-testid='reader']")[0].textContent).toBe("on");
    expect(host!.querySelectorAll("[data-testid='reader']")[1].textContent).toBe("off");
  });

  it("keeps unkeyed control local", async () => {
    await mount(<View />);
    await act(async () => toggle!());
    expect(text("view")).toBe("on");
    expect(surfaceControlStore.get("")).toBe(false);
  });

  it("releases control when the view unmounts", async () => {
    await mount(<View id="s1" />);
    await act(async () => toggle!());
    expect(surfaceControlStore.get("s1")).toBe(true);
    await act(async () => root!.render(<Reader id="s1" />));
    expect(surfaceControlStore.get("s1")).toBe(false);
    expect(text("reader")).toBe("off");
  });
});
