import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

const fetches = vi.hoisted(() => ({ localFetch: vi.fn() }));
vi.mock("../../lib/webApi", async (actual) => ({ ...await actual<object>(), ...fetches }));

import { FreeLimitPlansModal } from "./FreeLimitPlansModal";
import { checkoutTheme } from "./checkoutTheme";
import { freeLimitGate, upgradePlan } from "./freeLimitGate";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;

async function render(upgrade: (planKey: string) => Promise<void>) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<FreeLimitPlansModal upgrade={upgrade} />));
}

const dialog = () => document.querySelector<HTMLElement>("[role=dialog]");
const button = (name: string) => [...document.querySelectorAll("button")].find((candidate) => candidate.textContent === name)!;
const click = (name: string) => act(async () => { button(name).click(); });

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  act(() => freeLimitGate.close());
  freeLimitGate.setLimits(null);
  document.body.replaceChildren();
});

describe("plans modal", () => {
  it("stays closed until a limit opens it, then shows that limit with the daemon's maximum", async () => {
    await render(async () => {});
    expect(dialog()).toBeNull();
    freeLimitGate.setLimits({ enforcing: true, max: { workspaces: 3, teams: 6, "agent.tabs": 35 } });
    await act(async () => freeLimitGate.open("workspaces"));
    expect(dialog()?.querySelector("h2")?.textContent).toBe("The free version includes 3 workspaces.");
    expect(dialog()?.querySelectorAll("section[aria-label$=' plan']")).toHaveLength(4);
    expect(dialog()?.querySelector("section[aria-label='Enterprise plan']")).not.toBeNull();
  });

  it("Upgrade checks out pro_monthly and closes", async () => {
    const upgrade = vi.fn(async () => {});
    await render(upgrade);
    await act(async () => freeLimitGate.open("agent.tabs"));
    await click("Upgrade");
    expect(upgrade).toHaveBeenCalledWith("pro_monthly");
    expect(dialog()).toBeNull();
  });

  it("a chosen card checks out its own key", async () => {
    const upgrade = vi.fn(async () => {});
    await render(upgrade);
    await act(async () => freeLimitGate.open("teams"));
    await click("Choose Sponsor");
    expect(upgrade).toHaveBeenCalledWith("sponsor_yearly");
  });

  it("a checkout failure stays open and says why", async () => {
    await render(async () => { throw new Error("opaque-billing-failure"); });
    await act(async () => freeLimitGate.open("teams"));
    await click("Upgrade");
    expect(dialog()?.querySelector("[role=alert]")?.textContent).toContain("opaque-billing-failure");
  });

  it("Not now and Escape close it and return focus to where the user was", async () => {
    const origin = document.createElement("button");
    document.body.append(origin);
    origin.focus();
    await render(async () => {});
    await act(async () => freeLimitGate.open("workspaces"));
    expect(document.activeElement).not.toBe(origin);
    await click("Not now");
    expect(dialog()).toBeNull();
    expect(document.activeElement).toBe(origin);
    await act(async () => freeLimitGate.open("workspaces"));
    await act(async () => { dialog()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true })); });
    expect(dialog()).toBeNull();
  });

  it("upgradePlan posts the chosen key with the app's resolved theme and surfaces a refusal", async () => {
    document.documentElement.dataset.ctTheme = "light";
    fetches.localFetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await upgradePlan();
    const [path, init] = fetches.localFetch.mock.calls[0];
    expect(path).toBe("/api/account/upgrade");
    expect(JSON.parse(init.body)).toEqual({ plan: "pro_monthly", theme: "light" });
    document.documentElement.dataset.ctTheme = "dark";
    fetches.localFetch.mockResolvedValueOnce(new Response("refused", { status: 400 }));
    await expect(upgradePlan("sponsor_yearly")).rejects.toThrow("refused");
    expect(JSON.parse(fetches.localFetch.mock.calls[1][1].body)).toEqual({ plan: "sponsor_yearly", theme: "dark" });
  });

  it("checkout theme is light or dark, never system", () => {
    expect(checkoutTheme({ dataset: { ctTheme: "light" } })).toBe("light");
    expect(checkoutTheme({ dataset: { ctTheme: "dark" } })).toBe("dark");
    expect(checkoutTheme({ dataset: {} })).toBeNull();
    expect(checkoutTheme({ dataset: { ctTheme: "system" } })).toBeNull();
  });
});
