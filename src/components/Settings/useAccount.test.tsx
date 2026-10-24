import { act, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Notification } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
const accountEvents = vi.hoisted(() => new Map<string, Set<() => void>>());
vi.mock("../../lib/bridge", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/bridge")>(),
  listen: async (event: string, handler: () => void) => {
    const handlers = accountEvents.get(event) ?? new Set<() => void>();
    accountEvents.set(event, handlers);
    handlers.add(handler);
    return () => { handlers.delete(handler); };
  },
}));
const { request, rawBody, state } = vi.hoisted(() => ({
  request: vi.fn(), rawBody: vi.fn(() => "opaque-sensitive-response"), state: {
    identity:null as null | {uid:string;display_name:string;email:null;provider:string},
    failure:null as null | {stage:"redemption";error:"provider"},
    failLogin:false, failStatus:false, failPlan:false, failBillingAction:false, loginHeldElsewhere:false,
  },
}));
vi.mock("../../lib/webApi", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/webApi")>(),
  localFetch: async (url:string) => {
  request(url);
  if (url.endsWith("/status") && state.failStatus) throw new Error("opaque background status failure");
  if (url.endsWith("/plan") && state.failPlan) throw new Error("opaque background plan failure");
  if (url.endsWith("/portal") && state.failBillingAction) return {ok:false,text:rawBody};
  if (url.split("?")[0].endsWith("/login") && state.loginHeldElsewhere) return {ok:false,text:rawBody};
  if (url.endsWith("/status") && state.loginHeldElsewhere) return {ok:true,json:async () => ({identity:null,pending:true,last_sign_in_failure:null})};
  if (url.split("?")[0].endsWith("/login") && state.failLogin) {
    state.failure = {stage:"redemption",error:"provider"};
    return {ok:false,text:rawBody};
  }
  if (url.endsWith("/plan")) return {ok:true,json:async () => ({entitlements:{plan:"free",status:"active",current_period_end:0,cancel_at_period_end:false},stale:false})};
  if (url.split("?")[0].endsWith("/login")) state.identity = {uid:"user",display_name:"Ada",email:null,provider:"fake"};
  if (url.endsWith("/logout")) state.identity = null;
  return {ok:true,json:async () => ({identity:state.identity,pending:false,last_sign_in_failure:state.failure})};
  },
}));
import { signInFailureToastMessage, useAccount } from "./useAccount";
import { AccountTab } from "./AccountTab";
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT = true;

it.each([
  ["preparation", "sign-in setup failed"],
  ["browser", "opening the sign-in page failed"],
  ["callback", "the callback could not be validated"],
  ["redemption", "the code exchange failed"],
  ["credential_save", "saving sign-in credentials failed"],
  ["publication", "publishing the signed-in session failed"],
] as const)("names the %s stage and offers retry", (stage, expected) => {
  const message = signInFailureToastMessage({ stage, error: "provider" });
  expect(message).toContain(expected);
  expect(message).toContain("Retry");
});

function Surface({name}:{name:string}) {
  const {model,signIn,signOut} = useAccount();
  return <section aria-label={name}><output>{model.kind === "signed_in" ? model.identity.uid : model.kind}</output>
    <button type="button" onClick={() => {void signIn({provider:"email",mode:"signup"});}}>Login</button>
    <button type="button" onClick={() => {void signOut();}}>Logout</button>
  </section>;
}

function AccountTabWithToasts() {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = useMemo(() => ({
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
  }), []);
  return <MultiplexerActionsContext.Provider value={actions as unknown as MultiplexerActionsContextType}>
    <AccountTab />
    <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
  </MultiplexerActionsContext.Provider>;
}
it("refreshes mounted account surfaces on server identity changes and removes listeners on unmount", async () => {
  state.identity = {uid:"external-user",display_name:"Ada",email:null,provider:"fake"};
  state.failure = null;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<><Surface name="header" /><Surface name="canvas" /></>));
    expect(Array.from(host.querySelectorAll("output"), item => item.textContent)).toEqual(["external-user", "external-user"]);
    state.identity = null;
    await act(async () => { accountEvents.get("account-changed")?.forEach(handler => handler()); });
    expect(Array.from(host.querySelectorAll("output"), item => item.textContent)).toEqual(["signed_out", "signed_out"]);
  } finally {
    await act(async () => root.unmount()); host.remove(); state.identity = null;
  }
  expect(accountEvents.get("account-changed")?.size).toBe(0);
});

it("updates both mounted account surfaces after login and logout and guards duplicate login", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<><Surface name="titlebar" /><Surface name="settings" /></>));
    expect(Array.from(host.querySelectorAll("output"), item => item.textContent)).toEqual(["signed_out","signed_out"]);
    const buttons = host.querySelectorAll("button");
    await act(async () => {buttons[0].click();buttons[0].click();});
    expect(request.mock.calls.filter(([url]) => url.split("?")[0].endsWith("/login"))).toHaveLength(1);
    expect(request).toHaveBeenCalledWith("/api/account/login?provider=email&mode=signup");
    expect(Array.from(host.querySelectorAll("output"), item => item.textContent)).toEqual(["user","user"]);
    await act(async () => buttons[3].click());
    expect(Array.from(host.querySelectorAll("output"), item => item.textContent)).toEqual(["signed_out","signed_out"]);
  } finally {await act(async () => root.unmount());host.remove();}
});

it("retains a completed failure across surfaces, focus refresh and remount without reading the error body", async () => {
  state.identity = null; state.failure = null; state.failLogin = true;
  rawBody.mockClear();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const surfaces = <><Surface name="titlebar" /><Surface name="settings" /></>;
  const kinds = () => Array.from(host.querySelectorAll("output"), item => item.textContent);
  try {
    await act(async () => root.render(surfaces));
    await act(async () => host.querySelector("button")!.click());
    expect(kinds()).toEqual(["error", "error"]);
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(kinds()).toEqual(["error", "error"]);
    await act(async () => root.render(null));
    await act(async () => root.render(surfaces));
    expect(kinds()).toEqual(["error", "error"]);
    expect(rawBody).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount()); host.remove();
    state.identity = null; state.failure = null; state.failLogin = false;
  }
});

it("keeps failed background account and plan reads quiet while preserving user action errors", async () => {
  state.identity = {uid:"user",display_name:"Ada",email:null,provider:"fake"};
  state.failure = null;
  state.failLogin = false;
  state.failStatus = false;
  state.failPlan = true;
  state.failBillingAction = false;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTabWithToasts />));
    await act(async () => { await Promise.resolve(); });
    expect(host.querySelector('[data-testid="settings-copy-row"] code')?.textContent).toBe("user");
    expect(host.querySelector('[role="alert"]')).toBeNull();

    state.failStatus = true;
    await act(async () => window.dispatchEvent(new Event("focus")));
    await act(async () => { await Promise.resolve(); });
    expect(host.querySelector('[data-testid="settings-copy-row"] code')?.textContent).toBe("user");
    expect(host.querySelector('[role="alert"]')).toBeNull();

    state.failBillingAction = true;
    const manage = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Manage subscription");
    expect(manage).toBeDefined();
    await act(async () => manage!.click());
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
    state.identity = null; state.failure = null; state.failLogin = false; state.failStatus = false; state.failPlan = false; state.failBillingAction = false;
  }
});

it("shows a direct sign-in failure in a retryable toast", async () => {
  state.identity = null; state.failure = null; state.failLogin = true; state.failStatus = false; state.failPlan = false; state.failBillingAction = false;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTabWithToasts />));
    const signIn = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Sign in");
    expect(signIn).toBeDefined();
    await act(async () => signIn!.click());
    expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(Array.from(host.querySelectorAll("button")).some((button) => button.textContent === "Retry")).toBe(true);
    expect(Array.from(host.querySelectorAll("button")).some((button) => button.textContent === "Copy details")).toBe(true);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    state.identity = null; state.failure = null; state.failLogin = false;
  }
});

it("attaches to a sign-in another surface holds instead of reporting a failure", async () => {
  state.identity = null; state.failure = null; state.loginHeldElsewhere = false;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AccountTabWithToasts />));
    state.loginHeldElsewhere = true;
    const signIn = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Sign in");
    await act(async () => signIn!.click());
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="notification-toast"]')).toBeNull();
    state.loginHeldElsewhere = false;
    state.identity = {uid:"user",display_name:"Ada",email:null,provider:"fake"};
    await act(async () => window.dispatchEvent(new Event("domios:account-changed")));
    expect(host.querySelector('[data-testid="settings-copy-row"] code')?.textContent).toBe("user");
  } finally {
    await act(async () => root.unmount()); host.remove();
    state.identity = null; state.failure = null; state.loginHeldElsewhere = false;
  }
});
