import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import exportFixture from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/export.json";
import type { TeamTemplatesTransport } from "../../lib/transport/teamTemplates";
import { accountPlanModel, type AccountPlan } from "../Settings/accountPlanModel";
import { NeighborFilesFailure } from "../Neighbors/neighborFilesClient";
import { TemplateExportDialog, type NeighborShare } from "./TemplateExportDialog";

const account = vi.hoisted(() => ({ plan: vi.fn(), signIn: vi.fn(), refresh: vi.fn() }));
vi.mock("../Settings/useAccount", () => ({ useAccount: () => ({ model: { kind: "signed_in", identity: { uid: "buyer" } }, signIn: account.signIn }) }));
vi.mock("../Settings/useAccountPlan", () => ({ useAccountPlan: () => account.plan() }));

const paidPlan: AccountPlan = {
  entitlements: { plan: "pro", status: "active", current_period_end: 0, cancel_at_period_end: false },
  access: { authority: "billing", full_access: false }, stale: false,
};
const billing = (value: AccountPlan) => ({ plan: accountPlanModel(value), planLoad: "ready", pending: false, refresh: account.refresh });

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
  account.plan.mockReset().mockReturnValue(billing(paidPlan));
  account.signIn.mockReset().mockResolvedValue(null);
  account.refresh.mockReset().mockResolvedValue(null);
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

  it.each([
    ["sign_in_required", "This machine needs to sign in again before it can share", "Sign in again"],
    ["unconfirmed", "We couldn't confirm your plan right now", "Retry"],
  ] as const)("blocks %s despite the friends list reporting paid access", async (verification, message, action) => {
    account.plan.mockReturnValue(billing({ ...paidPlan, access: null, verification }));
    await renderDialog(true);
    expect(dialog().querySelector('[role="status"]')?.textContent).toContain(message);
    expect(exportButton().disabled).toBe(true);
    await act(async () => dialog().dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true })));
    expect(exportPackage).not.toHaveBeenCalled();
    expect(neighbors.send).not.toHaveBeenCalled();
    const recovery = Array.from(dialog().querySelectorAll<HTMLButtonElement>("button")).find(button => button.textContent === action)!;
    await act(async () => recovery.click());
    if (verification === "sign_in_required") expect(account.signIn).toHaveBeenCalledWith({ mode: "signin" });
    else expect(account.signIn).not.toHaveBeenCalled();
    expect(account.refresh).toHaveBeenCalledOnce();
    expect(exportPackage).not.toHaveBeenCalled();
  });

  it("says in plain words that it is sending until the send answers", async () => {
    let answer!: (receipt: { status: string }) => void;
    neighbors.send.mockReturnValue(new Promise((resolve) => { answer = resolve; }));
    await renderDialog(true);
    await chooseMira();
    await act(async () => exportButton().click());
    expect(dialog().querySelector('[role="status"]')?.textContent).toBe("Sending to Mira…");
    expect(dialog().querySelector('[role="alert"]')).toBeNull();
    expect(dialog().textContent).not.toContain("sent to Mira");
    await act(async () => answer({ status: "pending" }));
    expect(dialog().textContent).not.toContain("Sending to Mira");
    expect(dialog().textContent).toContain("sent to Mira");
  });

  it.each([["billing", false], ["admin", true]] as const)("lets a %s plan share whether or not it has staff full access", async (authority, full_access) => {
    account.plan.mockReturnValue(billing({ ...paidPlan, access: { authority, full_access } }));
    await renderDialog(true);
    expect(dialog().querySelector('[role="status"]')?.textContent ?? "").not.toContain("Settings > Account");
    await chooseMira();
    expect(exportButton().disabled).toBe(false);
    await act(async () => exportButton().click());
    expect(neighbors.send).toHaveBeenCalledWith("mira", "Shared the team Research desk 1.0.0", "/data/team-exports/research-desk.team");
  });

  it("blocks host free access even when the friends list reports paid", async () => {
    account.plan.mockReturnValue(billing({ ...paidPlan, access: { authority: "free", full_access: false } }));
    await renderDialog(true);
    expect(dialog().querySelector('[role="status"]')?.textContent).toContain("Upgrade in Settings > Account");
    expect(exportButton().disabled).toBe(true);
    await act(async () => dialog().dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true })));
    expect(exportPackage).not.toHaveBeenCalled();
  });

  it("keeps Share disabled until the host verification check finishes", async () => {
    account.plan.mockReturnValue({ plan: null, planLoad: "loading", pending: false, refresh: account.refresh });
    await renderDialog(true);
    expect(exportButton().disabled).toBe(true);
    expect(dialog().querySelector('[role="status"]')?.textContent).toContain("Checking sharing access");
    await act(async () => dialog().dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true })));
    expect(exportPackage).not.toHaveBeenCalled();
  });

  it.each([
    ["installation_unverified", "This machine needs to sign in again", "Sign in again"],
    ["entitlements_unavailable", "We couldn't confirm your plan", "Retry"],
    ["friends_locked", "Upgrade in Settings > Account", null],
    ["recipient_not_receiving", "Ask them to enable receiving", null],
    ["future_refusal", "try again later", null],
  ] as const)("explains a typed %s refusal without exposing its code", async (reason, message, action) => {
    neighbors.send.mockRejectedValue(new NeighborFilesFailure(reason, `${reason}: internal detail`, null));
    await renderDialog(true);
    await chooseMira();
    await act(async () => exportButton().click());
    expect(dialog().querySelector('[role="alert"]')?.textContent).toContain(message);
    expect(dialog().textContent).not.toContain(reason);
    expect(dialog().textContent).not.toContain("internal detail");
    if (action) expect(Array.from(dialog().querySelectorAll("button")).some(button => button.textContent === action)).toBe(true);
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
    expect(dialog().querySelector('[role="alert"]')?.textContent).toContain("try again later");
    expect(dialog().textContent).not.toContain("friends_locked");
    expect(dialog().textContent).not.toContain("sent to Mira");
  });

  it("does not claim an unconfirmed or refused share was sent", async () => {
    neighbors.send.mockResolvedValue({ status: "not_delivered", reason: "recipient_locked" });
    await renderDialog(true);
    await chooseMira();
    await act(async () => exportButton().click());
    expect(dialog().querySelector('[role="alert"]')?.textContent).toContain("Ask them to check their plan");
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
