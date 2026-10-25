import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { Notification } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { MultiplexerProvider, useMultiplexer, useMuxState } from "../../store/multiplexer";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
const { signIn, signOut, manage, openPlans, openUrl, state } = vi.hoisted(() => ({
  signIn: vi.fn(), signOut: vi.fn(), manage: vi.fn(), openPlans: vi.fn(), openUrl: vi.fn(),
  state: { signedIn: false, sessionOnly: false, billingPending: false, planStatus: "active", planAuthority: "billing", fullAccess: false, useRealPlan: false, photoUrl: null as string | null },
}));
const localFetch = vi.hoisted(() => vi.fn());
vi.mock("../Settings/useAccount", async (importOriginal) => ({
  ...await importOriginal<typeof import("../Settings/useAccount")>(),
  useAccount: () => ({model: state.signedIn
    ? {kind:"signed_in",sessionOnly:state.sessionOnly,identity:{uid:"user",display_name:"Ada",email:"ada@example.test",provider:"fake",photo_url:state.photoUrl}}
    : {kind:"signed_out"}, signIn, signOut}),
}));
vi.mock("../Settings/useAccountPlan", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../Settings/useAccountPlan")>();
  return { ...actual, useAccountPlan: (identityKey: string | null) => state.useRealPlan
    ? actual.useAccountPlan(identityKey)
    : ({
      plan:{authority:state.planAuthority,fullAccess:state.fullAccess,plan:"pro",status:state.planStatus,stale:false,periodEnd:null,cancelAtPeriodEnd:false},error:null,manage,
      pending:state.billingPending,
    }) };
});
vi.mock("../../lib/webApi", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/webApi")>(),
  localFetch,
}));
vi.mock("../../lib/nativeBridge", () => ({nativeBridge:{openUrl}}));
import { siteLinks } from "../../../shared/siteLinks";
import { AccountTray } from "./AccountTray";
import { AccountMeshClientProvider } from "../MeshPanel/accountMeshClient";
import { FakeAccountMeshClient, snapshot } from "../MeshPanel/accountMachines.testdata";
import { accountInitials } from "../Settings/accountModel";
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
    <AccountTray onOpenPlans={openPlans} />
    <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
  </MultiplexerActionsContext.Provider>;
}

function AccountTrayWithActions() {
  const actions = { upsertToast: vi.fn() } as unknown as MultiplexerActionsContextType;
  return <MultiplexerActionsContext.Provider value={actions}><AccountTray onOpenPlans={openPlans} /></MultiplexerActionsContext.Provider>;
}

function RealProviderToastStack() {
  const { toasts } = useMuxState();
  const { dismissToast } = useMultiplexer();
  return <NotificationToastContainer toasts={toasts} onDismiss={dismissToast} />;
}

function AccountTrayWithRealProvider() {
  return <MultiplexerProvider><AccountTray onOpenPlans={openPlans} /><RealProviderToastStack /></MultiplexerProvider>;
}

async function configureMultiplexerBridge() {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  localStorage.clear();
  sessionStorage.clear();
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "mesh_list_peers") return { paired: [] } as T;
    if (command === "list_notifications") return [] as T;
    return null as T;
  });
  __setListenForTest(async () => () => {});
}

async function resetMultiplexerBridge() {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  __setInvokeForTest(null);
  __setListenForTest(null);
}

it("opens the existing account menu and starts login once from its sign-in item", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTrayWithActions />));
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

it("renders sign-in failures through the real multiplexer provider and toast stack", async () => {
  signIn.mockResolvedValueOnce({ stage: "redemption", error: "provider" });
  await configureMultiplexerBridge();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTrayWithRealProvider />));
    const trigger = host.querySelector("button")!;
    await act(async () => trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    const signin = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find((item) => item.textContent === "Sign in");
    await act(async () => signin!.click());
    const toast = host.querySelector<HTMLElement>('[data-testid="notification-toast"]');
    expect(toast?.getAttribute("data-notification-id")).toBe("account-sign-in-failure");
    expect(toast?.getAttribute("role")).toBe("status");
  } finally {
    await act(async () => root.unmount()); host.remove();
    await resetMultiplexerBridge();
  }
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

it("renders billing portal failures through the real multiplexer provider and toast stack", async () => {
  state.signedIn = true;
  state.useRealPlan = true;
  localFetch.mockImplementation(async (url: string) => url === "/api/account/plan"
    ? { ok: true, json: async () => ({ entitlements: { plan: "pro", status: "active", current_period_end: 0, cancel_at_period_end: false }, access: { authority: "billing", full_access: false }, stale: false }) }
    : { ok: false, text: async () => "No billing customer" });
  await configureMultiplexerBridge();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTrayWithRealProvider />));
    const trigger = host.querySelector("button")!;
    await act(async () => trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    const manageItem = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]'))
      .find((item) => item.textContent?.includes("Manage subscription"));
    await act(async () => manageItem!.click());
    expect(localFetch).toHaveBeenCalledWith("/api/account/portal", expect.objectContaining({ method: "POST" }));
    const toast = host.querySelector<HTMLElement>('[data-testid="notification-toast"]');
    expect(toast?.getAttribute("data-notification-id")).toBe("account-billing-action-failure");
    expect(toast?.getAttribute("role")).toBe("status");
  } finally {
    await act(async () => root.unmount()); host.remove();
    await resetMultiplexerBridge();
    state.signedIn = false;
    state.useRealPlan = false;
    localFetch.mockReset();
  }
});

it("says the subscription service is unavailable when the portal answers 503", async () => {
  state.signedIn = true;
  state.useRealPlan = true;
  localFetch.mockImplementation(async (url: string) => url === "/api/account/plan"
    ? { ok: true, status: 200, json: async () => ({ entitlements: { plan: "sponsor", status: "active", current_period_end: 0, cancel_at_period_end: false }, access: { authority: "billing", full_access: false }, stale: false }) }
    : { ok: false, status: 503, text: async () => "Billing request refused" });
  await configureMultiplexerBridge();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTrayWithRealProvider />));
    await act(async () => host.querySelector("button")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    const manageItem = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]'))
      .find((item) => item.textContent?.includes("Manage subscription"));
    await act(async () => manageItem!.click());
    const toast = host.querySelector<HTMLElement>('[data-testid="notification-toast"]');
    expect(toast?.textContent).toContain("Subscription service unavailable");
    expect(toast?.textContent).toContain("Try again later");
    expect(toast?.textContent).not.toContain("Billing request refused");
  } finally {
    await act(async () => root.unmount()); host.remove();
    await resetMultiplexerBridge();
    state.signedIn = false;
    state.useRealPlan = false;
    localFetch.mockReset();
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
    await act(async () => root.render(<AccountTrayWithActions />));
    await act(async () => host.querySelector("button")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    const head = document.querySelector<HTMLElement>('[data-testid="account-menu-head"]')!;
    expect(head.querySelector(".ct-plan-badge")?.textContent).toBe("Admin · Pro");
    expect(head.getAttribute("aria-label")).toContain("Admin · Pro · active");
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
    await act(async () => root.render(<AccountTrayWithActions />));
    const button = host.querySelector("button");
    expect(button?.getAttribute("aria-label")).toBe("Account: Ada");
    await act(async () => button!.dispatchEvent(new KeyboardEvent("keydown", {key:"Enter",bubbles:true})));
    const menu = document.querySelector('[role="menu"]');
    const head = menu!.querySelector<HTMLElement>('[data-testid="account-menu-head"]')!;
    expect(head.textContent).toContain("ada@example.test");
    expect(head.querySelector(".ct-plan-badge")?.textContent).toBe("Pro");
    expect(head.querySelector(".ct-plan-status")?.textContent).toBe("active");
    expect(menu?.getAttribute("aria-label")).toBe("Account: Ada, Pro · active");
    expect(head.closest('[role="menuitem"], button')).toBeNull();
    expect(head.querySelector('[role="menuitem"], button, [tabindex]')).toBeNull();
    expect(Array.from(menu!.querySelectorAll<HTMLElement>('[role="menuitem"]')).some((item) => item.textContent?.includes("ada@example.test"))).toBe(false);
    const manageItem = Array.from(menu!.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent?.includes("Manage subscription"));
    await act(async () => manageItem!.click());
    expect(manage).toHaveBeenCalledExactlyOnceWith(openPlans);
    await act(async () => button!.dispatchEvent(new KeyboardEvent("keydown", {key:"Enter",bubbles:true})));
    const signOutItem = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent === "Sign out");
    await act(async () => signOutItem!.click());
    expect(signOut).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); state.signedIn = false; }
});

it("marks the tray busy and disables Manage subscription while billing opens", async () => {
  state.signedIn = true;
  state.billingPending = true;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTrayWithActions />));
    const button = host.querySelector("button")!;
    expect(button.getAttribute("aria-busy")).toBe("true");
    await act(async () => button.dispatchEvent(new KeyboardEvent("keydown", {key:"Enter",bubbles:true})));
    const item = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(entry => entry.textContent?.includes("Opening subscription"));
    expect(item instanceof HTMLButtonElement && item.disabled).toBe(true);
    await act(async () => item!.click());
    expect(manage).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); state.signedIn = false; state.billingPending = false; }
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
    await act(async () => root.render(<AccountTrayWithActions />));
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
    await act(async () => root.render(<AccountTrayWithActions />));
    for (const [label,url] of [["Telegram",siteLinks.telegram],["Discord",siteLinks.discord]]) {
      await act(async () => host.querySelector("button")!.dispatchEvent(new KeyboardEvent("keydown",{key:"Enter",bubbles:true})));
      const item = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent === label);
      await act(async () => item!.click());
      expect(openUrl).toHaveBeenLastCalledWith(url);
    }
    expect(openUrl).toHaveBeenCalledTimes(2);
  } finally {await act(async () => root.unmount());host.remove();state.signedIn=false;}
});

for (const signedIn of [false,true]) it(`offers terms and privacy links only when signed in (signedIn=${signedIn})`, async () => {
  state.signedIn = signedIn;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTrayWithActions />));
    for (const [label,url] of [["Terms of Service",siteLinks.terms],["Privacy Policy",siteLinks.privacy]]) {
      await act(async () => host.querySelector("button")!.dispatchEvent(new KeyboardEvent("keydown",{key:"Enter",bubbles:true})));
      const item = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent === label);
      if (!signedIn) { expect(item).toBeUndefined(); continue; }
      await act(async () => item!.click());
      expect(openUrl).toHaveBeenLastCalledWith(url);
    }
    expect(openUrl).toHaveBeenCalledTimes(signedIn ? 2 : 0);
  } finally {await act(async () => root.unmount());host.remove();state.signedIn=false;}
});

const PHOTO = "https://lh3.googleusercontent.com/a/fixture=s96-c";
async function renderSignedIn(photoUrl: string | null) {
  state.signedIn = true;
  state.photoUrl = photoUrl;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<AccountTrayWithActions />));
  const button = host.querySelector("button")!;
  const cleanup = async () => { await act(async () => root.unmount()); host.remove(); state.signedIn = false; state.photoUrl = null; };
  return { button, cleanup };
}
async function openMenuHeader(button: HTMLButtonElement) {
  await act(async () => button.dispatchEvent(new KeyboardEvent("keydown", {key:"Enter",bubbles:true})));
  return document.querySelector<HTMLElement>('[data-testid="account-menu-head"]')!;
}

it("shows the provider avatar in the tray and menu header without sending a referrer", async () => {
  const { button, cleanup } = await renderSignedIn(PHOTO);
  try {
    const image = button.querySelector("img")!;
    expect(image.getAttribute("src")).toBe(PHOTO);
    expect(image.getAttribute("alt")).toBe("Ada avatar");
    expect(image.getAttribute("referrerpolicy")).toBe("no-referrer");
    expect(button.getAttribute("aria-label")).toBe("Account: Ada");
    const header = await openMenuHeader(button);
    expect(header.querySelector("img")?.getAttribute("src")).toBe(PHOTO);
    expect(header.querySelector("img")?.getAttribute("referrerpolicy")).toBe("no-referrer");
  } finally { await cleanup(); }
});

it("falls back to the initial when the avatar fails to load", async () => {
  const { button, cleanup } = await renderSignedIn(PHOTO);
  try {
    await act(async () => button.querySelector("img")!.dispatchEvent(new Event("error")));
    expect(button.querySelector("img")).toBeNull();
    expect(button.textContent).toBe(accountInitials("Ada"));
    const header = await openMenuHeader(button);
    await act(async () => header.querySelector("img")!.dispatchEvent(new Event("error")));
    expect(header.querySelector("img")).toBeNull();
    expect(header.textContent).toContain(accountInitials("Ada"));
  } finally { await cleanup(); }
});

for (const photoUrl of [null, "http://lh3.googleusercontent.com/a/fixture", `https://lh3.googleusercontent.com/${"a".repeat(2048)}`]) {
  it(`shows the initial without an image for photo_url=${photoUrl?.slice(0, 40) ?? "none"}`, async () => {
    const { button, cleanup } = await renderSignedIn(photoUrl);
    try {
      expect(button.querySelector("img")).toBeNull();
      expect(button.textContent).toBe(accountInitials("Ada"));
      expect((await openMenuHeader(button)).querySelector("img")).toBeNull();
    } finally { await cleanup(); }
  });
}

it("exposes the session-only storage warning in the account menu", async () => {
  state.signedIn = true;
  state.sessionOnly = true;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTrayWithActions />));
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

it("keeps normal sign-out and confirms self session revocation separately", async () => {
  state.signedIn = true;
  const client = new FakeAccountMeshClient(snapshot());
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const openMenu = () => act(async () => host.querySelector("button")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
  const chooseRevoke = async () => {
    const items = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    expect(items.some((item) => item.textContent === "Sign out")).toBe(true);
    await act(async () => items.find((item) => item.textContent === "Sign out this machine")!.click());
  };
  try {
    await act(async () => root.render(<AccountMeshClientProvider value={client}><AccountTrayWithActions /></AccountMeshClientProvider>));
    await openMenu();
    await chooseRevoke();
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    expect(client.sessionRevokeCalls).toEqual([]);
    await act(async () => [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find((button) => button.textContent === "Cancel")!.click());
    expect(client.sessionRevokeCalls).toEqual([]);
    await openMenu();
    await chooseRevoke();
    await act(async () => document.querySelector<HTMLButtonElement>('[role="dialog"] button[type="submit"]')!.click());
    expect(client.sessionRevokeCalls).toEqual([undefined]);
    expect(signOut).not.toHaveBeenCalled();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  } finally {
    await act(async () => root.unmount()); host.remove(); state.signedIn = false;
  }
});
