import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
const { signIn, signOut, manage, state } = vi.hoisted(() => ({
  signIn: vi.fn(), signOut: vi.fn(), manage: vi.fn(),
  state: { signedIn: false },
}));
vi.mock("../Settings/useAccount", () => ({useAccount: () => ({model: state.signedIn
  ? {kind:"signed_in",identity:{uid:"user",display_name:"Ada",email:"ada@example.test",provider:"fake"}}
  : {kind:"signed_out"}, signIn, signOut})}));
vi.mock("../Settings/useAccountPlan", () => ({useAccountPlan: () => ({
  plan:{plan:"pro",status:"active",stale:false,periodEnd:null,cancelAtPeriodEnd:false},error:null,manage,
})}));
import { AccountTray } from "./AccountTray";
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT = true;
it("offers an accessible sign-in button and calls login once", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTray />));
    const button = host.querySelector("button");
    expect(button?.getAttribute("aria-label")).toBe("Sign in");
    await act(async () => button!.click());
    expect(signIn).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});
it("shows identity and plan and wires subscription and sign out", async () => {
  state.signedIn = true;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTray />));
    const button = host.querySelector("button");
    expect(button?.getAttribute("aria-label")).toBe("Account: Ada");
    await act(async () => button!.dispatchEvent(new KeyboardEvent("keydown", {key:"Enter",bubbles:true})));
    const menu = document.querySelector('[role="menu"]');
    expect(menu?.textContent).toContain("ada@example.test");
    expect(menu?.textContent).toContain("pro (active)");
    const manageItem = Array.from(menu!.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent?.includes("Manage subscription"));
    await act(async () => manageItem!.click());
    expect(manage).toHaveBeenCalledTimes(1);
    await act(async () => button!.dispatchEvent(new KeyboardEvent("keydown", {key:"Enter",bubbles:true})));
    const signOutItem = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent?.includes("Sign out"));
    await act(async () => signOutItem!.click());
    expect(signOut).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); state.signedIn = false; }
});
