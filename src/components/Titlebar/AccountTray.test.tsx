import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { Notification } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
const { signIn, signOut, manage, openUrl, state } = vi.hoisted(() => ({
  signIn: vi.fn(), signOut: vi.fn(), manage: vi.fn(), openUrl: vi.fn(),
  state: { signedIn: false, sessionOnly: false, planStatus: "active", planAuthority: "billing", fullAccess: false },
}));
vi.mock("../Settings/useAccount", async (importOriginal) => ({
  ...await importOriginal<typeof import("../Settings/useAccount")>(),
  useAccount: () => ({model: state.signedIn
    ? {kind:"signed_in",sessionOnly:state.sessionOnly,identity:{uid:"user",display_name:"Ada",email:"ada@example.test",provider:"fake"}}
    : {kind:"signed_out"}, signIn, signOut}),
}));
vi.mock("../Settings/useAccountPlan", () => ({useAccountPlan: () => ({
  plan:{authority:state.planAuthority,fullAccess:state.fullAccess,plan:"pro",status:state.planStatus,stale:false,periodEnd:null,cancelAtPeriodEnd:false},error:null,manage,
})}));
vi.mock("../../lib/nativeBridge", () => ({nativeBridge:{openUrl}}));
import { siteLinks } from "../../../shared/siteLinks";
import { AccountTray } from "./AccountTray";
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => {vi.clearAllMocks();});

function AccountTrayWithToasts() {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = {
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id),
      { ...toast, timestamp: 1, read: false },
    ]),
  } as unknown as MultiplexerActionsContextType;
  return <MultiplexerActionsContext.Provider value={actions}>
    <AccountTray />
    <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
  </MultiplexerActionsContext.Provider>;
}

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

it("shows a stage-named toast with a retry after sign-in fails", async () => {
  signIn.mockResolvedValueOnce({ stage: "redemption", error: "provider" });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTrayWithToasts />));
    const trigger = host.querySelector("button")!;
    await act(async () => trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    const item = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find((entry) => entry.textContent === "Sign in");
    await act(async () => item!.click());
    const toast = host.querySelector<HTMLElement>('[data-testid="notification-toast"]');
    expect(toast?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    const retry = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Retry");
    expect(retry).toBeDefined();
    await act(async () => retry!.click());
    expect(signIn).toHaveBeenCalledTimes(2);
    expect(signIn).toHaveBeenNthCalledWith(2, { mode: "signin" });
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("shows subscription failures in a retryable toast without a titlebar alert", async () => {
  state.signedIn = true;
  manage.mockResolvedValueOnce(new Error("billing_portal_failed"));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTrayWithToasts />));
    const trigger = host.querySelector("button")!;
    await act(async () => trigger.dispatchEvent(new KeyboardEvent("keydown", {key:"Enter",bubbles:true})));
    const manageItem = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]'))
      .find((item) => item.textContent?.includes("Manage subscription"));
    await act(async () => manageItem!.click());
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    const retry = Array.from(host.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent === "Retry");
    expect(retry).toBeDefined();
    await act(async () => retry!.click());
    expect(manage).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount()); host.remove();
    state.signedIn = false;
  }
});

it("reports unavailable subscription management without offering a crashing retry", async () => {
  state.signedIn = true;
  state.planStatus = "unavailable";
  manage.mockResolvedValueOnce(new Error("functions URL is unavailable"));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTrayWithToasts />));
    const trigger = host.querySelector("button")!;
    await act(async () => trigger.dispatchEvent(new KeyboardEvent("keydown", {key:"Enter",bubbles:true})));
    const manageItem = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]'))
      .find((item) => item.textContent?.includes("Manage subscription"));
    await act(async () => manageItem!.click());
    const toast = host.querySelector('[data-testid="notification-toast"]');
    expect(toast?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(Array.from(host.querySelectorAll("button")).some((button) => button.textContent === "Retry")).toBe(false);
  } finally {
    await act(async () => root.unmount()); host.remove();
    state.signedIn = false; state.planStatus = "active";
  }
});

it("shows the Admin plan line from server-authoritative billing state", async () => {
  state.signedIn = true;
  state.planAuthority = "admin";
  state.fullAccess = true;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTray />));
    await act(async () => host.querySelector("button")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    expect(document.querySelector('[role="menu"]')?.textContent).toContain("Admin · pro (active)");
  } finally {
    await act(async () => root.unmount()); host.remove();
    state.signedIn = false; state.planAuthority = "billing"; state.fullAccess = false;
  }
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

it("exposes the session-only storage warning in the account menu", async () => {
  state.signedIn = true;
  state.sessionOnly = true;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTray />));
    await act(async () => host.querySelector("button")!.dispatchEvent(new KeyboardEvent("keydown", {key:"Enter",bubbles:true})));
    const warning = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]'))
      .find(item => item.textContent?.includes("Signed in for this session only"));
    expect(warning).toBeDefined();
    expect(warning instanceof HTMLButtonElement && warning.disabled).toBe(true);
  } finally {
    await act(async () => root.unmount()); host.remove();
    state.signedIn = false; state.sessionOnly = false;
  }
});
