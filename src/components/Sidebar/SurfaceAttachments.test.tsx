import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SURFACE_PULSE_MS, recordSurfaceActivity, surfaceActivityStore } from "../../lib/surfaceActivity";
import { surfaceControlStore } from "../../lib/surfaceControlStore";
import type { SurfaceTabBacking, Tab } from "../../lib/types";
import { SurfaceOwnerBlock } from "./SurfaceAttachments";
import { attachmentCollapseStore } from "./surfaceAttachmentModel";
import type { AttachmentToggle } from "./TabItem";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const backing: SurfaceTabBacking = { kind: "surface", plugin_id: "browser", surface_id: "page", instance_id: "i", owner_label: true };
const surface = (id: string): Tab => ({ id, title: "Browser", tab_type: "Terminal", pty_id: null, cwd: null, backing, parent_tab_id: "snake" });

let root: Root | null = null;
let host: HTMLElement | null = null;
let lastToggle: AttachmentToggle | null = null;

function fakeRect(el: Element): DOMRect {
  const box = (left: number, top: number, width: number, height: number) => ({ left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) }) as DOMRect;
  if (el.matches("[data-testid='surface-owner-block']")) return box(0, 0, 200, 80);
  if (el.matches("[data-testid='tab-tab-icon']")) {
    const row = el.closest("[data-tether-attachment]");
    if (!row) return box(16, 8, 12, 12);
    const index = [...row.parentElement!.children].indexOf(row);
    return box(30, 34 + index * 20, 11, 11);
  }
  return box(0, 0, 0, 0);
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    return fakeRect(this);
  });
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
  lastToggle = null;
  surfaceActivityStore.reset();
  surfaceControlStore.reset();
  attachmentCollapseStore.reset();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function Block({ attachments, compact = false }: { attachments: Tab[]; compact?: boolean }) {
  return (
    <SurfaceOwnerBlock
      ownerId="snake"
      attachments={attachments}
      compact={compact}
      renderOwner={(toggle) => {
        lastToggle = toggle;
        return <div data-testid="owner-row"><span data-testid="tab-tab-icon" /></div>;
      }}
      renderAttachment={(tab) => <div data-testid="attachment-row" data-id={tab.id}><span data-testid="tab-tab-icon" /></div>}
    />
  );
}

async function mount(attachments: Tab[], compact = false) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<Block attachments={attachments} compact={compact} />));
}

const all = (testId: string) => [...host!.querySelectorAll(`[data-testid='${testId}']`)];

describe("surface owner block", () => {
  it("renders the owner alone when it has no surfaces", async () => {
    await mount([]);
    expect(all("surface-owner-block")).toHaveLength(0);
    expect(all("owner-row")).toHaveLength(1);
    expect(lastToggle).toBeNull();
  });

  it("tucks surfaces under the owner and draws one faint idle strand each", async () => {
    await mount([surface("s1"), surface("s2")]);
    expect(all("attachment-row").map((row) => row.getAttribute("data-id"))).toEqual(["s1", "s2"]);
    const strands = all("surface-tether");
    expect(strands.map((strand) => strand.getAttribute("data-state"))).toEqual(["idle", "idle"]);
    expect(strands[0].getAttribute("d")).toBe("M22 22.5C22 31 28.75 39.5 35.5 39.5");
    expect(strands[1].getAttribute("d")).toBe("M22 22.5C22 41 28.75 59.5 35.5 59.5");
    expect(all("surface-tether-clearing").map((disc) => disc.getAttribute("cy"))).toEqual(["39.5", "59.5"]);
    expect(all("surface-tether-droplet")).toHaveLength(0);
  });

  it("always shows the owner's surfaces with no collapse toggle", async () => {
    await mount([surface("s1"), surface("s2")]);
    expect(lastToggle).toBeNull();
    expect(all("attachment-row")).toHaveLength(2);
  });

  it("drops the tether in the narrow sidebar", async () => {
    await mount([surface("s1")], true);
    expect(all("attachment-row")).toHaveLength(1);
    expect(all("surface-tether")).toHaveLength(0);
  });

  it("runs the droplet once per agent verb", async () => {
    await mount([surface("s1"), surface("s2")]);
    await act(async () => recordSurfaceActivity({ tab_id: "s1", verb: "click" }));
    expect(all("surface-tether-droplet")).toHaveLength(1);
    expect(all("surface-tether").map((strand) => strand.getAttribute("data-state"))).toEqual(["driving", "idle"]);
    await act(async () => vi.advanceTimersByTime(SURFACE_PULSE_MS));
    expect(all("surface-tether-droplet")).toHaveLength(0);
    expect(all("surface-tether")[0].getAttribute("data-state")).toBe("idle");
  });

  it("goes solid and still while the user has Take control", async () => {
    await mount([surface("s1")]);
    await act(async () => surfaceControlStore.set("s1", true));
    await act(async () => recordSurfaceActivity({ tab_id: "s1", verb: "click" }));
    expect(all("surface-tether")[0].getAttribute("data-state")).toBe("controlled");
    expect(all("surface-tether-droplet")).toHaveLength(0);
  });
});
