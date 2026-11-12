import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../../store/multiplexer/hooks", () => ({
  useMuxState: () => ({ workspaces: [{ id: "general", protected: true }, { id: "project", protected: false }] }),
}));
vi.mock("./NewSessionPicker", async () => {
  const { useState } = await import("react");
  let mounts = 0;
  return {
    NewSessionPicker: ({ workspaceId, section, onClose }: { workspaceId: string; section: string; onClose: () => void }) => {
      const [mount] = useState(() => ++mounts);
      return <button type="button" data-testid="picker" data-workspace={workspaceId} data-section={section} data-mount={mount} onClick={onClose}>picker</button>;
    },
  };
});

import { openNewSessionPicker } from "../../lib/newSessionChoice";
import { takePendingTemplatePreview } from "../../lib/teamTemplates/pendingPreview";
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

function openedFiles() {
  const stop = vi.fn();
  let deliver: ((path: string) => void) | null = null;
  const subscribe = vi.fn(async (handler: (path: string) => void) => { deliver = handler; return stop; });
  return { files: { subscribe }, stop, subscribe, open: (path: string) => act(async () => deliver!(path)) };
}

it("opens a package the OS handed over in the ordinary workspace's review, replacing a dialog already open", async () => {
  const opened = openedFiles();
  await act(async () => root.render(<NewSessionPickerHost firstRunActive={false} openedTeamFiles={opened.files} />));
  await act(async () => openNewSessionPicker({ workspaceId: "project", workspaceCwd: "/p" }));
  const before = picker()!.dataset.mount;
  await opened.open("/downloads/desk.team");
  expect(picker()?.dataset).toMatchObject({ workspace: "project", section: "workspace" });
  expect(picker()!.dataset.mount).not.toBe(before);
  expect(takePendingTemplatePreview()).toEqual({ file: "/downloads/desk.team" });
});

it("leaves an opened package waiting natively until first run ends, then stops listening on unmount", async () => {
  const opened = openedFiles();
  await act(async () => root.render(<NewSessionPickerHost firstRunActive openedTeamFiles={opened.files} />));
  expect(opened.subscribe).not.toHaveBeenCalled();
  await act(async () => root.render(<NewSessionPickerHost firstRunActive={false} openedTeamFiles={opened.files} />));
  expect(opened.subscribe).toHaveBeenCalledOnce();
  await act(async () => root.unmount());
  expect(opened.stop).toHaveBeenCalledOnce();
  root = createRoot(host);
});
