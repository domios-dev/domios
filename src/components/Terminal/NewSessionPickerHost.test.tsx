import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../../store/multiplexer/hooks", () => ({
  useMuxState: () => ({ workspaces: [{ id: "general", protected: true }, { id: "project", protected: false }] }),
}));
vi.mock("./NewSessionPicker", () => ({
  NewSessionPicker: ({ workspaceId, section, onClose }: { workspaceId: string; section: string; onClose: () => void }) =>
    <button type="button" data-testid="picker" data-workspace={workspaceId} data-section={section} onClick={onClose}>picker</button>,
}));

import { openNewSessionPicker } from "../../lib/newSessionChoice";
import { NewSessionPickerHost } from "./NewSessionPickerHost";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root;
let host: HTMLDivElement;
const picker = () => host.querySelector<HTMLButtonElement>('[data-testid="picker"]');

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("opens the session dialog from any caller, independent of the sidebar, and closes it on request", async () => {
  await act(async () => root.render(<NewSessionPickerHost firstRunActive={false} />));
  expect(picker()).toBeNull();
  await act(async () => openNewSessionPicker({ workspaceId: "general", workspaceCwd: null }));
  expect(picker()?.dataset).toMatchObject({ workspace: "general", section: "general" });
  await act(async () => openNewSessionPicker({ workspaceId: "project", workspaceCwd: "/p" }));
  expect(picker()?.dataset.section).toBe("workspace");
  await act(async () => picker()!.click());
  expect(picker()).toBeNull();
});

it("stays closed and drops an open dialog during first run", async () => {
  await act(async () => root.render(<NewSessionPickerHost firstRunActive={false} />));
  await act(async () => openNewSessionPicker({ workspaceId: "project", workspaceCwd: null }));
  await act(async () => root.render(<NewSessionPickerHost firstRunActive />));
  expect(picker()).toBeNull();
  await act(async () => openNewSessionPicker({ workspaceId: "project", workspaceCwd: null }));
  expect(picker()).toBeNull();
});
