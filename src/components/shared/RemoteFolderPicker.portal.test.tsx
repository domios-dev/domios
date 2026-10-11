import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { RemoteFolderPicker } from "./RemoteFolderPicker";

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn().mockResolvedValue({ path: "/home", home: "/home", roots: [], entries: [] }),
}));
vi.mock("./BusyState", () => ({ BusyState: () => null }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => document.body.replaceChildren());

it("opens the folder dialog through the viewport portal and selects the loaded folder", async () => {
  const drawer = document.createElement("div");
  document.body.append(drawer);
  const root = createRoot(drawer);
  const onSelect = vi.fn();
  const onCancel = vi.fn();
  try {
    await act(async () => root.render(<RemoteFolderPicker title="Workspace folder" onSelect={onSelect} onCancel={onCancel} />));
    const dialog = document.querySelector('[role="dialog"][aria-label="Workspace folder"]');
    expect(dialog).not.toBeNull();
    expect(drawer.contains(dialog)).toBe(false);
    const buttons = Array.from(dialog!.querySelectorAll("button"));
    await act(async () => buttons.find((button) => button.textContent === "Select Folder")!.click());
    expect(onSelect).toHaveBeenCalledExactlyOnceWith("/home");
    await act(async () => dialog!.querySelector<HTMLButtonElement>('[aria-label="Close folder picker"]')!.click());
    expect(onCancel).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
  }
});
