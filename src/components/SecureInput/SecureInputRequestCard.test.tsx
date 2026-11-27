import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import {
  SecureInputRequestCard,
  type SecureInputRequestView,
} from "./SecureInputRequestCard";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function sampleRequest(
  overrides: Partial<SecureInputRequestView> = {},
): SecureInputRequestView {
  return {
    requestId: "sir_ui_test_1",
    targetInstallationId: "inst-cloud-rollacodebot",
    targetMachineName: "rollacodebot",
    isRemote: true,
    consumer: { type: "vault_slot", name: "github-release-pat", overwrite: true },
    targetLabel: "vault:github-release-pat",
    kind: "token",
    reason: "Push release tags from cloud worker",
    status: "pending",
    createdAtMs: 1_000,
    expiresAtMs: 301_000,
    ...overrides,
  };
}

function setInputValue(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("SecureInputRequestCard", () => {
  it("renders a masked password field, wipes value immediately on submit, and locks after delivery", async () => {
    const secret = "ghp_SUPER_SECRET_UI_TOKEN_771";
    let resolveSubmit!: (value: SecureInputRequestView) => void;
    const onSubmit = vi.fn(
      (id: string, value: string, inst: string) =>
        new Promise<SecureInputRequestView>((resolve) => {
          expect(id).toBe("sir_ui_test_1");
          expect(value).toBe(secret);
          expect(inst).toBe("inst-cloud-rollacodebot");
          resolveSubmit = resolve;
        }),
    );
    const onRefuse = vi.fn(async () => {});

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <SecureInputRequestCard
            request={sampleRequest()}
            onSubmit={onSubmit}
            onRefuse={onRefuse}
          />,
        );
      });

      const input = host.querySelector(
        'input[aria-label="Secret value"]',
      ) as HTMLInputElement;
      expect(input).not.toBeNull();
      expect(input.type).toBe("password");
      expect(input.getAttribute("autocomplete")).toBe("off");

      await act(async () => {
        setInputValue(input, secret);
      });
      const form = host.querySelector("form")!;
      await act(async () => {
        form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      });

      // Field must be wiped immediately before async submission settles.
      expect(input.value).toBe("");
      expect(onSubmit).toHaveBeenCalledTimes(1);

      await act(async () => {
        resolveSubmit(sampleRequest({ status: "delivered" }));
      });

      const status = host.querySelector('[role="status"]');
      expect(status?.textContent).toMatch(/delivered directly to vault:github-release-pat/i);
      expect(host.textContent).not.toContain(secret);
      expect(host.querySelector('input[aria-label="Secret value"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("locks on uncertain failure without replaying and scrubs the secret from error text", async () => {
    const secret = "sk-ant-SECRET-UNCERTAIN-994";
    const onSubmit = vi.fn(async () => {
      throw new Error(`transport failed for ${secret}`);
    });
    const onRefuse = vi.fn(async () => {});

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <SecureInputRequestCard
            request={sampleRequest({
              consumer: { type: "provider_key", provider_id: "anthropic" },
              targetLabel: "provider-key:anthropic",
              kind: "key",
            })}
            onSubmit={onSubmit}
            onRefuse={onRefuse}
          />,
        );
      });

      const input = host.querySelector(
        'input[aria-label="Secret value"]',
      ) as HTMLInputElement;
      await act(async () => {
        setInputValue(input, secret);
      });
      const form = host.querySelector("form")!;
      await act(async () => {
        form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      });

      const status = host.querySelector('[role="status"]');
      expect(status?.textContent).toMatch(/uncertain/i);
      expect(onSubmit).toHaveBeenCalledTimes(1);
      expect(host.textContent).not.toContain(secret);
      expect(host.querySelector('input[aria-label="Secret value"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("allows owner to refuse a pending request without submitting any secret", async () => {
    const onSubmit = vi.fn(async () => sampleRequest({ status: "delivered" }));
    const onRefuse = vi.fn(async () => {});

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <SecureInputRequestCard
            request={sampleRequest()}
            onSubmit={onSubmit}
            onRefuse={onRefuse}
          />,
        );
      });

      const refuseBtn = Array.from(host.querySelectorAll("button")).find(
        (btn) => btn.textContent === "Refuse",
      )!;
      await act(async () => {
        refuseBtn.click();
      });

      expect(onRefuse).toHaveBeenCalledWith("sir_ui_test_1");
      expect(onSubmit).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
