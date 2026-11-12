import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import exchange from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/exchange.json";
import type { TeamTemplatesTransport } from "../../lib/transport/teamTemplates";
import { OPEN_NEW_SESSION_EVENT } from "../../lib/newSessionChoice";
import { takePendingTemplatePreview } from "../../lib/teamTemplates/pendingPreview";

const mux = vi.hoisted(() => ({ workspaces: [{ id: "general", cwd: null, protected: true }, { id: "workspace", cwd: "/workspace", protected: false }], active_workspace_id: "general" }));
vi.mock("../../store/multiplexer/hooks", () => ({ useMuxSelector: (select: (state: typeof mux) => unknown) => select(mux) }));

import { TemplateShareCardView } from "./TemplateShareCardView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const card = vi.fn();
const importPackage = vi.fn();
const start = vi.fn();
const transport = () => ({ card, import: importPackage, preview: start, distill: start, export: start, delete: start } as unknown as TeamTemplatesTransport);
const resolvePath = vi.fn(async () => "/neighbors/mira/research-desk.team");
let root: Root | null = null;
let host: HTMLDivElement;
let onDismiss = vi.fn();

beforeEach(() => {
  card.mockReset().mockResolvedValue(exchange.share_card);
  importPackage.mockReset().mockResolvedValue(exchange.import_result);
  start.mockReset();
  onDismiss = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  host.remove();
  takePendingTemplatePreview();
});

async function renderCard() {
  await act(async () => {
    root!.render(<TemplateShareCardView friendship="mira" fileId="f_91" senderName="Mira" onDismiss={onDismiss} resolvePath={resolvePath} transport={transport} />);
  });
}

const button = (text: string) => Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((candidate) => candidate.textContent?.trim() === text)!;

describe("Neighborhood team share card", () => {
  it("shows the checked package as unverified and offers only Preview and Dismiss", async () => {
    await renderCard();
    expect(card).toHaveBeenCalledWith("f_91", "/neighbors/mira/research-desk.team", expect.anything());
    const view = host.querySelector('[data-testid="template-share-card"]')!;
    expect(view.getAttribute("aria-label")).toBe("Mira shared a team");
    expect(view.querySelector('[data-testid="template-share-status"]')?.textContent).toBe("Unverified");
    expect(view.textContent).toContain("Asks for 1 secret. Runs a script on start.");
    expect(Array.from(view.querySelectorAll("button")).map((candidate) => candidate.textContent?.trim())).toEqual(["Preview", "Dismiss"]);
  });

  it("previews by importing to My templates and opening its review, never starting anything", async () => {
    const opened = vi.fn();
    window.addEventListener(OPEN_NEW_SESSION_EVENT, opened);
    await renderCard();
    await act(async () => button("Preview").click());
    window.removeEventListener(OPEN_NEW_SESSION_EVENT, opened);
    expect(importPackage).toHaveBeenCalledWith("/neighbors/mira/research-desk.team");
    expect(start).not.toHaveBeenCalled();
    expect((opened.mock.calls[0][0] as CustomEvent).detail).toMatchObject({ workspaceId: "workspace", section: "workspace" });
    expect(takePendingTemplatePreview()).toEqual({ template: { name: "ada/research-desk", version: "1.1.0" }, update: exchange.import_result.update });
    expect(host.querySelector('[data-testid="template-share-status"]')?.textContent).toBe("Previewed");
  });

  it("dismisses without importing", async () => {
    await renderCard();
    await act(async () => button("Dismiss").click());
    expect(onDismiss).toHaveBeenCalledOnce();
    expect(importPackage).not.toHaveBeenCalled();
  });
});
