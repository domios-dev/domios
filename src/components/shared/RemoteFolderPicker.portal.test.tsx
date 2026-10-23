import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "../../lib/bridge";
import { dismissTopSurface } from "../../lib/backDismissal";
import { RemoteFolderPicker } from "./RemoteFolderPicker";

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "fs_mkdir") throw new Error("Invalid folder name");
    return { path: "/home", home: "/home", roots: [], entries: [{ name: "src", path: "/home/src" }] };
  }),
}));
vi.mock("./BusyState", () => ({ BusyState: () => null }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => document.body.replaceChildren());

async function renderPicker(onSelect = vi.fn(), onCancel = vi.fn()) {
  const drawer = document.createElement("div");
  document.body.append(drawer);
  const root = createRoot(drawer);
  await act(async () => root.render(<RemoteFolderPicker title="Workspace folder" onSelect={onSelect} onCancel={onCancel} />));
  const dialog = document.querySelector('[role="dialog"][aria-label="Workspace folder"]')!;
  const button = (label: string) => Array.from(dialog.querySelectorAll("button")).find((entry) => entry.textContent === label);
  return { drawer, root, dialog, button };
}

it("opens the folder dialog through the viewport portal and selects the loaded folder", async () => {
  const onSelect = vi.fn();
  const onCancel = vi.fn();
  const { drawer, root, dialog, button } = await renderPicker(onSelect, onCancel);
  try {
    expect(dialog).not.toBeNull();
    expect(drawer.contains(dialog)).toBe(false);
    await act(async () => button("Select Folder")!.click());
    expect(onSelect).toHaveBeenCalledExactlyOnceWith("/home");
    await act(async () => dialog.querySelector<HTMLButtonElement>('[aria-label="Close folder picker"]')!.click());
    expect(onCancel).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
  }
});

it("shows a refused folder name beside the form and keeps the listing", async () => {
  const { root, dialog, button } = await renderPicker();
  try {
    await act(async () => dialog.querySelector<HTMLButtonElement>('[data-testid="folder-picker-new"]')!.click());
    const name = dialog.querySelector<HTMLInputElement>('[data-testid="folder-picker-new-name"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(name, "bad:name");
      name.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => dialog.querySelector<HTMLButtonElement>('[data-testid="folder-picker-new-create"]')!.click());
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("fs_mkdir", { parent: "/home", name: "bad:name" });
    expect(dialog.textContent).toContain("Invalid folder name");
    expect(dialog.textContent).not.toContain("reading this folder");
    expect(button("src")).toBeDefined();
    expect(dialog.querySelector('[data-testid="folder-picker-new-name"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
  }
});

it("system Back closes the open folder picker", async () => {
  const onCancel = vi.fn();
  const { root } = await renderPicker(vi.fn(), onCancel);
  try {
    await act(async () => { dismissTopSurface(); });
    expect(onCancel).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
  }
});
