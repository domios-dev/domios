import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
const { signIn, signOut, manage, openUrl, state } = vi.hoisted(() => ({
  signIn: vi.fn(), signOut: vi.fn(), manage: vi.fn(), openUrl: vi.fn(),
  state: { signedIn: false },
}));
vi.mock("../Settings/useAccount", () => ({useAccount: () => ({model: state.signedIn
  ? {kind:"signed_in",identity:{uid:"user",display_name:"Ada",email:"ada@example.test",provider:"fake"}}
  : {kind:"signed_out"}, signIn, signOut})}));
vi.mock("../Settings/useAccountPlan", () => ({useAccountPlan: () => ({
  plan:{plan:"pro",status:"active",stale:false,periodEnd:null,cancelAtPeriodEnd:false},error:null,manage,
})}));
vi.mock("../../lib/nativeBridge", () => ({nativeBridge:{openUrl}}));
import { siteLinks } from "../../../shared/siteLinks";
import { AccountTray } from "./AccountTray";
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => {vi.clearAllMocks();});
it("opens the existing account menu and starts login once from its sign-in item", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTray />));
    const button = host.querySelector("button");
    expect(button?.getAttribute("aria-label")).toBe("Account");
    await act(async () => button!.dispatchEvent(new KeyboardEvent("keydown", {key:"Enter",bubbles:true})));
    const items = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]'));
    expect(items.some(item => item.textContent === "Continue with Apple")).toBe(false);
    const signin = items.find(item => item.textContent === "Sign in");
    await act(async () => signin!.click());
    expect(signIn).toHaveBeenCalledTimes(1);
    expect(signIn).toHaveBeenCalledWith({mode:"signin"});
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

for (const [label, hint] of [
  ["Create account",{mode:"signup"}],
  ["Continue with Google",{provider:"google",mode:"signin"}],
  ["Continue with Facebook",{provider:"facebook",mode:"signin"}],
  ["Continue with email",{provider:"email",mode:"signin"}],
] as const) it(`passes the browser hint for ${label}`, async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTray />));
    await act(async () => host.querySelector("button")!.dispatchEvent(new KeyboardEvent("keydown",{key:"Enter",bubbles:true})));
    const item = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent === label);
    await act(async () => item!.click());
    expect(signIn).toHaveBeenCalledExactlyOnceWith(hint);
  } finally {await act(async () => root.unmount());host.remove();}
});
for (const signedIn of [false,true]) it(`opens community links in the system browser while signedIn=${signedIn}`, async () => {
  state.signedIn = signedIn;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTray />));
    for (const [label,url] of [["Telegram",siteLinks.telegram],["Discord",siteLinks.discord]]) {
      await act(async () => host.querySelector("button")!.dispatchEvent(new KeyboardEvent("keydown",{key:"Enter",bubbles:true})));
      const item = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent === label);
      await act(async () => item!.click());
      expect(openUrl).toHaveBeenLastCalledWith(url);
    }
    expect(openUrl).toHaveBeenCalledTimes(2);
  } finally {await act(async () => root.unmount());host.remove();state.signedIn=false;}
});
