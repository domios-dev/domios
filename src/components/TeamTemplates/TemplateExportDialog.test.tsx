import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import exportFixture from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/export.json";
import type { TeamTemplatesTransport } from "../../lib/transport/teamTemplates";
import { TemplateExportDialog, type NeighborShare } from "./TemplateExportDialog";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const summary = { name: "me/research-desk", version: "1.0.0", title: "Research desk", description: "Researches.", provenance: "saved" as const, workers: 2, verified: true };
const exported = { ...exportFixture.result, path: "/data/team-exports/research-desk.team" };
const exportPackage = vi.fn();
const transport = () => ({ export: exportPackage } as unknown as TeamTemplatesTransport);
const friend = (id: string, receiving = true) => ({ uid: id, displayName: id === "mira" ? "Mira" : "Sam", photoURL: null, friendship_id: id, receiving, since: 0, inbound_per_hour: 10, outbound_per_hour: 10 });
const list = (access: "paid" | "locked" = "paid", receiving = true) => ({ schema_version: 1 as const, access, friends: [friend("mira", receiving), friend("sam", receiving)], incoming: [], outgoing: [] });
const neighbors = { friends: vi.fn(), send: vi.fn() } satisfies NeighborShare;
let root: Root | null = null;
let host: HTMLDivElement;

beforeEach(() => {
  exportPackage.mockReset().mockResolvedValue(exported);
  neighbors.friends.mockReset().mockResolvedValue(list());
  neighbors.send.mockReset().mockResolvedValue({ status: "pending" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  host.remove();
});

const dialog = () => document.body.querySelector<HTMLElement>('[role="dialog"]')!;
const exportButton = () => Array.from(dialog().querySelectorAll<HTMLButtonElement>("button")).find((candidate) => ["Export", "Share"].includes(candidate.textContent?.trim() ?? ""))!;

async function renderDialog(share: boolean) {
  await act(async () => {
    root!.render(<TemplateExportDialog summary={summary} share={share} onClose={vi.fn()} transport={transport} neighbors={neighbors} />);
  });
}

async function chooseMira() {
  const recipient = dialog().querySelector<HTMLElement>('[aria-label^="Share with"]')!;
  await act(async () => recipient.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, cancelable: true })));
  const mira = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitemradio"]')).find((item) => item.textContent?.trim() === "Mira")!;
  await act(async () => mira.click());
}

describe("Export with Neighborhood sharing", () => {
  it("exports the package and hands the written file to the ordinary Neighborhood send", async () => {
    await renderDialog(true);
    expect(dialog().querySelector("h2")?.textContent).toBe("Share Research desk");
    expect(exportButton().textContent).toBe("Share");
    expect(exportButton().disabled).toBe(true);
    expect(dialog().querySelector('[aria-label^="Share with"]')?.textContent).toContain("Choose a recipient");
    await act(async () => dialog().dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true })));
    expect(exportPackage).not.toHaveBeenCalled();
    expect(neighbors.send).not.toHaveBeenCalled();
    await chooseMira();
    expect(exportButton().disabled).toBe(false);
    await act(async () => exportButton().click());
    expect(exportPackage).toHaveBeenCalledWith({ template: { name: "me/research-desk", version: "1.0.0" }, format: "team" });
    expect(neighbors.send).toHaveBeenCalledWith("mira", "Shared the team Research desk 1.0.0", "/data/team-exports/research-desk.team");
    expect(dialog().textContent).toContain("sent to Mira");
  });

  it("blocks a locked channel before exporting and explains the next step", async () => {
    neighbors.friends.mockResolvedValue(list("locked"));
    await renderDialog(true);
    expect(exportButton().disabled).toBe(true);
    expect(dialog().querySelector('[role="status"]')?.textContent).toContain("Settings > Account");
    expect(dialog().textContent).not.toContain("friends_locked");
    await act(async () => dialog().dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true })));
    expect(exportPackage).not.toHaveBeenCalled();
    expect(neighbors.send).not.toHaveBeenCalled();
  });

  it.each(["not receiving", "paused", "files blocked"])("blocks a recipient who is %s", async (state) => {
    const response = list();
    Object.assign(response.friends[0], state === "not receiving" ? { receiving: false } : state === "paused" ? { their_paused: true } : { their_file_daily_limit_bytes: 0 });
    neighbors.friends.mockResolvedValue(response);
    await renderDialog(true);
    await chooseMira();
    expect(exportButton().disabled).toBe(true);
    expect(dialog().querySelector('[role="status"]')?.textContent).toContain("Ask them");
    await act(async () => dialog().dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true })));
    expect(exportPackage).not.toHaveBeenCalled();
  });

  it("offers only portable formats for sharing", async () => {
    await renderDialog(true);
    expect(Array.from(dialog().querySelectorAll<HTMLInputElement>('input[name="template-format"]')).map(input => input.value)).toEqual(["yaml", "team"]);
  });

  it("replaces a selected folder with a portable format when sharing is enabled", async () => {
    await renderDialog(false);
    await act(async () => dialog().querySelector<HTMLInputElement>('input[value="folder"]')!.click());
    await act(async () => dialog().querySelector<HTMLButtonElement>('[aria-label="Also share to Neighborhood"]')!.click());
    expect(dialog().querySelector('input[value="folder"]')).toBeNull();
    await chooseMira();
    await act(async () => exportButton().click());
    expect(exportPackage.mock.calls[0][0].format).toBe("team");
  });

  it("keeps raw errors out of sharing failures", async () => {
    neighbors.send.mockRejectedValue(new Error("friends_locked: Upgrade to use your Neighbors channel"));
    await renderDialog(true);
    await chooseMira();
    await act(async () => exportButton().click());
    expect(dialog().querySelector('[role="alert"]')?.textContent).toContain("Settings > Account");
    expect(dialog().textContent).not.toContain("friends_locked");
    expect(dialog().textContent).not.toContain("sent to Mira");
  });

  it("does not claim an unconfirmed or refused share was sent", async () => {
    neighbors.send.mockResolvedValue({ status: "not_delivered", reason: "recipient_locked" });
    await renderDialog(true);
    await chooseMira();
    await act(async () => exportButton().click());
    expect(dialog().querySelector('[role="alert"]')?.textContent).toContain("could not be confirmed");
    expect(dialog().textContent).not.toContain("sent to Mira");
    expect(dialog().textContent).not.toContain("recipient_locked");
  });

  it("exports a built-in in the chosen format without offering Neighborhood sharing", async () => {
    const builtin = { name: "dev.domios/default", version: "1.0.0", title: "Default team", description: "Default.", provenance: "builtin" as const, workers: 0, verified: false };
    await act(async () => {
      root!.render(<TemplateExportDialog summary={builtin} share onClose={vi.fn()} transport={transport} neighbors={neighbors} />);
    });
    expect(dialog().querySelector('[aria-label="Also share to Neighborhood"]')).toBeNull();
    await act(async () => dialog().querySelector<HTMLInputElement>('input[name="template-format"][value="folder"]')!.click());
    await act(async () => exportButton().click());
    expect(exportPackage).toHaveBeenCalledWith({ template: { name: "dev.domios/default", version: "1.0.0" }, format: "folder" });
    expect(neighbors.friends).not.toHaveBeenCalled();
    expect(neighbors.send).not.toHaveBeenCalled();
  });

  it("opens with focus on the chosen format, not the first one", async () => {
    await renderDialog(false);
    const focused = document.activeElement as HTMLInputElement;
    expect(focused.name).toBe("template-format");
    expect(focused.checked).toBe(true);
    expect(focused.value).toBe("team");
  });

  it("exports without sending anything when sharing stays off", async () => {
    await renderDialog(false);
    await act(async () => exportButton().click());
    expect(exportPackage).toHaveBeenCalledOnce();
    expect(neighbors.friends).not.toHaveBeenCalled();
    expect(neighbors.send).not.toHaveBeenCalled();
  });
});
