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
const neighbors = { friends: vi.fn(), send: vi.fn() } satisfies NeighborShare;
let root: Root | null = null;
let host: HTMLDivElement;

beforeEach(() => {
  exportPackage.mockReset().mockResolvedValue(exported);
  neighbors.friends.mockReset().mockResolvedValue([{ friendship: "mira", name: "Mira" }, { friendship: "sam", name: "Sam" }]);
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
const exportButton = () => Array.from(dialog().querySelectorAll<HTMLButtonElement>("button")).find((candidate) => candidate.textContent?.trim() === "Export")!;

async function renderDialog(share: boolean) {
  await act(async () => {
    root!.render(<TemplateExportDialog summary={summary} share={share} onClose={vi.fn()} transport={transport} neighbors={neighbors} />);
  });
}

describe("Export with Neighborhood sharing", () => {
  it("exports the package and hands the written file to the ordinary Neighborhood send", async () => {
    await renderDialog(true);
    expect(dialog().querySelector('[role="switch"][aria-label="Also share to Neighborhood"]')?.getAttribute("aria-checked")).toBe("true");
    await act(async () => exportButton().click());
    expect(exportPackage).toHaveBeenCalledWith({ template: { name: "me/research-desk", version: "1.0.0" }, format: "team" });
    expect(neighbors.send).toHaveBeenCalledWith("mira", "Shared the team Research desk 1.0.0", "/data/team-exports/research-desk.team");
    expect(dialog().textContent).toContain("sent to Mira");
  });

  it("exports without sending anything when sharing stays off", async () => {
    await renderDialog(false);
    await act(async () => exportButton().click());
    expect(exportPackage).toHaveBeenCalledOnce();
    expect(neighbors.friends).not.toHaveBeenCalled();
    expect(neighbors.send).not.toHaveBeenCalled();
  });
});
