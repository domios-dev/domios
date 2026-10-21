import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
const state = vi.hoisted(() => ({ error: "opaque failure" as string | null, busy: false, items: [], rows: [] }));
vi.mock("../../hooks/useUpdatePanel", () => ({ useUpdatePanel: () => state }));
vi.mock("../UpdatePanel", () => ({ UpdatePanel: () => <div>fixture details</div> }));
import { UpdateTray } from "./UpdateTray";
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
it("expires failure attention, acknowledges on opening, and resets after recovery", async () => {
  vi.useFakeTimers();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const render = () => act(async () => root.render(<UpdateTray />));
  const trigger = () => host.querySelector<HTMLButtonElement>('button[aria-label="Update"]')!;
  try {
    await render();
    expect(trigger().textContent).toContain("Update failed");
    await act(async () => vi.advanceTimersByTime(10_000));
    expect(trigger().textContent).toBe("Updates");
    expect(trigger().title).toBe("Update failed");
    state.error = "second failure"; await render();
    expect(trigger().textContent).toContain("Update failed");
    await act(async () => trigger().click());
    expect(trigger().textContent).toBe("Updates");
    expect(document.querySelector('[role="dialog"][aria-label="Updates"]')).not.toBeNull();
    await act(async () => trigger().click());
    state.error = null; await render();
    expect(trigger()).toBeNull();
    state.error = "third failure"; await render();
    expect(trigger().textContent).toContain("Update failed");
  } finally { await act(async () => root.unmount()); host.remove(); vi.useRealTimers(); }
});
