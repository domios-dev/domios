import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { CloudApiError } from "../../lib/cloudMachines/client";
import type { CloudReceipt, CloudUsagePayload } from "../../lib/cloudMachines/types";
import { recordProductEvent } from "../../lib/productAnalytics";
import { CloudTopUpDialog } from "../billing/CloudTopUpDialog";
import { CloudMachineDrawer } from "./CloudMachineDrawer";
import { CloudMachineSlot } from "./CloudMachineSlot";

vi.mock("../../lib/productAnalytics", async actual => ({
  ...(await actual<object>()),
  recordProductEvent: vi.fn(async () => {}),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const USAGE: CloudUsagePayload = {
  used: 6.2,
  allowance: 10,
  purchased: 35,
  remaining: 38.8,
  cap: 50,
  burn_per_size: { small: 1, large: 2 },
  currency: "USD",
  period_end_at: Date.UTC(2026, 11, 1) / 1000,
  custom_min_cents: 500,
  custom_max_cents: 50000,
  plan: "pro",
  purchased_expiry_days: 365,
  sizes: [
    { id: "small", cpu: 2, ram_gb: 4, hourly_rate_cents: 20, credits_per_hour: 1 },
    { id: "large", cpu: 4, ram_gb: 8, hourly_rate_cents: 40, credits_per_hour: 2 },
  ],
  packs: [
    { pack_id: "pack_5", price_cents: 500, hours: 25 },
    { pack_id: "pack_10", price_cents: 1000, hours: 50 },
    { pack_id: "pack_25", price_cents: 2500, hours: 125 },
  ],
  allowance_hours: { free: 0, pro: 10, max: 40, sponsor: 120 },
};

const MACHINE: CloudReceipt = {
  kind: "cloud",
  machine_id: "cm_1",
  name: "Scratch",
  provider: "fly_machines",
  size: "small",
  rate_per_hour: { amount: 0.2, currency: "USD", credits_per_hour: 1 },
  state: "awake",
  sleep_mode: "stop_billed",
  uptime_secs: 1800,
  billing_notice: "",
};

const hosts: HTMLDivElement[] = [];
afterEach(() => {
  while (hosts.length) hosts.pop()!.remove();
  vi.mocked(recordProductEvent).mockClear();
});

function createHost() {
  const host = document.createElement("div");
  document.body.append(host);
  hosts.push(host);
  return { host, root: createRoot(host) };
}

function setInputValue(input: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}

it("CloudMachineSlot offers Cloud machine and One of your computers with zero provider or key inputs", async () => {
  const onCreate = vi.fn();
  const onPair = vi.fn();
  const onCancel = vi.fn();
  const { host, root } = createHost();
  try {
    await act(async () =>
      root.render(
        <CloudMachineSlot
          usage={{ ...USAGE, allowance: 0, purchased: 0, remaining: 0, used: 0, plan: "free" }}
          isFree
          onCreateCloud={onCreate}
          onPairComputer={onPair}
          onCancel={onCancel}
        />,
      ),
    );
    expect(host.querySelectorAll("input")).toHaveLength(0);
    expect(host.textContent).not.toMatch(/Timeweb|Fly|API key|token/i);
    expect(host.querySelector('[data-testid="lock-badge"]')).not.toBeNull();
    expect(host.textContent).toContain("$0.20/h Small");

    const createBtn = Array.from(host.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Create machine")!;
    await act(async () => createBtn.click());
    expect(onCreate).toHaveBeenCalledWith("small");

    const pairRadio = Array.from(host.querySelectorAll('[role="radio"]')).find((el) => el.textContent?.includes("One of your computers")) as HTMLButtonElement;
    await act(async () => pairRadio.click());
    const continueBtn = Array.from(host.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Continue")!;
    await act(async () => continueBtn.click());
    expect(onPair).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
  }
});

it("CloudTopUpDialog supports presets, custom amount validation, wake CTA, and Free paywall", async () => {
  const startCheckout = vi.fn(async () => ({
    url: "https://checkout.stripe.test/c/pay_123",
    session_id: "cs_test_123",
    pack_id: "custom",
    credits: 75,
    amount_cents: 1500,
    currency: "usd",
  }));
  const openUrl = vi.fn();
  const onCheckoutStarted = vi.fn();
  const onClose = vi.fn();
  const { root } = createHost();
  try {
    await act(async () =>
      root.render(
        <CloudTopUpDialog
          mode="topup"
          usage={USAGE}
          currentPlan="pro"
          purchases
          onClose={onClose}
          onCheckoutStarted={onCheckoutStarted}
          startCheckout={startCheckout}
          openUrl={openUrl}
        />,
      ),
    );
    const dialog = document.querySelector<HTMLFormElement>('[role="dialog"]')!;
    expect(dialog.textContent).toContain("Top up cloud hours");
    expect(dialog.textContent).toContain("Recommended");
    expect(dialog.querySelector('[data-testid="topup-balance-preview"]')?.textContent).toContain("38.8 h");
    expect(dialog.querySelector('[data-testid="topup-balance-preview"]')?.textContent).toContain("88.8 h");

    const customRadio = dialog.querySelector<HTMLElement>('[role="radio"][aria-label="Custom amount"]')!;
    await act(async () => customRadio.click());
    const input = dialog.querySelector<HTMLInputElement>('input[aria-label="Custom amount in whole dollars"]')!;
    expect(document.activeElement).toBe(input);

    await act(async () => setInputValue(input, "2"));
    expect(dialog.querySelector('[role="alert"]')?.textContent).toBe("Enter a whole dollar amount from $5 to $500.");
    const submitBtn = dialog.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    expect(submitBtn.disabled).toBe(true);
    expect(submitBtn.textContent?.trim()).toBe("Add hours");

    await act(async () => setInputValue(input, "15"));
    expect(dialog.querySelector('[role="alert"]')).toBeNull();
    expect(submitBtn.disabled).toBe(false);
    expect(submitBtn.textContent?.trim()).toBe("Add 75 h for $15");

    await act(async () => {
      dialog.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    expect(startCheckout).toHaveBeenCalledWith({ pack_id: "custom", amount_cents: 1500 });
    expect(openUrl).toHaveBeenCalledWith("https://checkout.stripe.test/c/pay_123");
    expect(onCheckoutStarted).toHaveBeenCalledTimes(1);
    expect(onCheckoutStarted).toHaveBeenCalledWith(
      expect.objectContaining({ mode: "topup", packId: "custom", amountCents: 1500 }),
      expect.objectContaining({ pack_id: "custom", amount_cents: 1500 }),
    );
    expect(recordProductEvent).toHaveBeenCalledExactlyOnceWith({
      event_type: "cloud_paywall_shown",
      event_properties: { mode: "topup" },
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
  }
});

it("CloudTopUpDialog renders Free paywall with Pro comparison and wake mode with machine name", async () => {
  const onOpenPlans = vi.fn();
  const { root } = createHost();
  try {
    await act(async () =>
      root.render(
        <CloudTopUpDialog
          mode="free"
          usage={{ ...USAGE, allowance: 0, purchased: 0, remaining: 0, used: 0 }}
          currentPlan="free"
          purchases
          onClose={() => {}}
          onOpenPlans={onOpenPlans}
        />,
      ),
    );
    const dialog = document.querySelector<HTMLFormElement>('[role="dialog"]')!;
    expect(dialog.textContent).toContain("Start your cloud machine");
    expect(dialog.textContent).toContain("Or include 10 h every month with Pro");
    const viewPro = Array.from(dialog.querySelectorAll("button")).find((b) => b.textContent?.trim() === "View Pro")!;
    await act(async () => viewPro.click());
    expect(onOpenPlans).toHaveBeenCalledTimes(1);

    await act(async () =>
      root.render(
        <CloudTopUpDialog
          mode="wake"
          usage={{ ...USAGE, remaining: 0, purchased: 0 }}
          currentPlan="pro"
          machineId="cm_1"
          machineName="Scratch"
          purchases
          onClose={() => {}}
        />,
      ),
    );
    const wakeDialog = document.querySelector<HTMLFormElement>('[role="dialog"]')!;
    expect(wakeDialog.textContent).toContain("Top up to wake Scratch");
    expect(wakeDialog.querySelector<HTMLButtonElement>('button[type="submit"]')?.textContent?.trim()).toBe("Pay $10 and wake Scratch");
    expect(recordProductEvent).toHaveBeenCalledWith({
      event_type: "cloud_paywall_shown",
      event_properties: { mode: "free" },
    });
    expect(recordProductEvent).toHaveBeenCalledWith({
      event_type: "cloud_paywall_shown",
      event_properties: { mode: "wake" },
    });
  } finally {
    await act(async () => root.unmount());
  }
});

it("CloudMachineDrawer wires inline Redeem code states, config idle sleep, and delete unpushed-work confirmation", async () => {
  const onRedeem = vi.fn()
    .mockRejectedValueOnce(new CloudApiError({ status: 400, code: "expired", message: "Promo code has expired" }))
    .mockResolvedValueOnce({
      schema_version: 1,
      status: "redeemed",
      already_redeemed: false,
      hours: 10,
      expires_at: null,
      remaining: 48.8,
      purchased: 45,
    });
  const onDelete = vi.fn(async () => true);
  const onClose = vi.fn();
  const { host, root } = createHost();
  try {
    await act(async () =>
      root.render(
        <CloudMachineDrawer
          machine={MACHINE}
          usage={USAGE}
          planId="pro"
          idleSleepMinutes={15}
          onClose={onClose}
          onTopUp={() => {}}
          onRedeem={onRedeem}
          onSleep={async () => {}}
          onWake={async () => {}}
          onDelete={onDelete}
        />,
      ),
    );
    expect(host.textContent).toContain("After 15 min idle");
    expect(host.querySelector('[data-testid="cloud-usage-remaining"]')?.textContent).toBe("38.8 h");
    expect(host.querySelector('input[aria-label="Promo code"]')).toBeNull();

    const redeemToggle = Array.from(host.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Redeem code")!;
    await act(async () => redeemToggle.click());
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Promo code"]')!;
    await act(async () => setInputValue(input, "launch24"));
    const form = input.closest("form")!;
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("This code has expired.");
    expect(input.value).toBe("launch24");

    await act(async () => setInputValue(input, "launch25"));
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    expect(onRedeem).toHaveBeenLastCalledWith("LAUNCH25");
    expect(host.querySelector('input[aria-label="Promo code"]')).toBeNull();
    expect(host.querySelector('[role="status"]')?.textContent).toContain("Added 10 h from LAUNCH25.");

    const deleteLink = Array.from(host.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Delete machine")!;
    await act(async () => deleteLink.click());
    expect(onDelete).not.toHaveBeenCalled();
    expect(host.textContent).toContain("unpushed work");
    const confirmDelete = Array.from(host.querySelectorAll<HTMLButtonElement>('[role="group"] button')).find((b) => b.textContent?.trim() === "Delete machine")!;
    await act(async () => confirmDelete.click());
    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
  }
});
