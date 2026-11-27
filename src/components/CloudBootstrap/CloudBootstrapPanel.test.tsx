import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CloudBootstrapSnapshot } from "../../lib/cloudBootstrap/types";

const prepareMock = vi.fn();
const timingMock = vi.fn();

vi.mock("../../lib/cloudBootstrap/client", () => ({
  getCloudBootstrapStatus: vi.fn(),
  prepareCloudProviderSignIn: (...args: unknown[]) => prepareMock(...args),
  recordCloudProviderSignInTiming: (...args: unknown[]) => timingMock(...args),
}));

vi.mock("../Terminal/RuntimeLoginActions", () => ({
  RuntimeLoginActions: (props: {
    tabId: string;
    providerId?: string;
    state: {
      loginProviderName?: string;
      login?: { status?: string; userCode?: string; url?: string };
      loginBinding?: { nonce?: string; epoch?: number };
    };
    onLoginReply?: (reply: unknown) => void;
  }) => (
    <div
      data-testid="runtime-login-card"
      data-tab-id={props.tabId}
      data-provider-id={props.providerId}
      data-user-code={props.state.login?.userCode ?? ""}
      data-binding-nonce={props.state.loginBinding?.nonce ?? ""}
    >
      LoginCard:{props.state.loginProviderName}
      {props.state.login?.userCode ? `:${props.state.login.userCode}` : ""}
      {props.onLoginReply && (
        <button
          type="button"
          data-testid={`mock-reply-${props.providerId}`}
          onClick={() =>
            props.onLoginReply?.({
              login: {
                status: "pending",
                method: "device",
                loginId: "opaque-flow-1",
                userCode: "OPAQ-7741",
                url: "https://example.invalid/opaque-device",
              },
              loginBinding: { nonce: "opaque-nonce-1", epoch: 1 },
            })
          }
        >
          Mint opaque code
        </button>
      )}
    </div>
  ),
}));

import { CloudBootstrapPanel } from "./CloudBootstrapPanel";

describe("CloudBootstrapPanel", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    prepareMock.mockReset();
    timingMock.mockReset();
    timingMock.mockResolvedValue({ machine_id: "cloud-1", create_phases: {} });
  });

  afterEach(async () => {
    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  it("mounts existing RuntimeLoginActions for signed-out supported providers and keeps ready/unsupported truthful", async () => {
    const snapshot: CloudBootstrapSnapshot = {
      machine_id: "cloud-1",
      name: "warm-box",
      state: "awake",
      peer_id: "peer-cloud-1",
      create_phases: {
        requested_at_ms: 1000,
        peer_ready_at_ms: 12000,
        seed_started_at_ms: 12000,
        warm_console_ready_at_ms: 14500,
        warm_bootstrap_delta_secs: 2.5,
        within_budget: true,
      },
      warm_bootstrap: {
        schema_version: 1,
        status: "partial",
        creation_generation: "cloud-1:req-1",
        readable_owner_context: true,
        wake_up_verified: true,
        applied_counts: {
          identity_fields: 1,
          wake_up_items: 2,
          rules: 1,
          skills: 1,
          preference_fields: 2,
        },
        excluded: [{ category: "wake_up_item", reason: "secret_shaped_content" }],
        warnings: [],
        providers: [
          {
            provider_id: "claude",
            display_name: "Claude",
            state: "sign_in_required",
            supported_methods: ["browser"],
            code_input: true,
            login_tab_id: "tab-claude",
            requires_owner_input: true,
          },
          {
            provider_id: "acp",
            profile_id: "antigravity",
            display_name: "ACP (antigravity)",
            state: "sign_in_required",
            supported_methods: ["browser"],
            code_input: true,
            requires_owner_input: true,
          },
          {
            provider_id: "codex",
            display_name: "Codex",
            state: "ready",
            supported_methods: ["device"],
            code_input: false,
            requires_owner_input: false,
          },
          {
            provider_id: "custom-cli",
            display_name: "Custom CLI",
            state: "unsupported",
            supported_methods: [],
            code_input: false,
            requires_owner_input: false,
            unsupported_reason: "login_flow_unsupported",
          },
        ],
      },
    };

    prepareMock.mockResolvedValueOnce({
      providers: [
        {
          provider_id: "acp",
          profile_id: "antigravity",
          display_name: "ACP (antigravity)",
          state: "sign_in_required",
          supported_methods: ["browser"],
          code_input: true,
          tab_id: "tab-ag",
          remote_tab_id: "mesh:peer-cloud-1/tab-ag",
          card_ready: true,
          requires_owner_input: true,
        },
      ],
    });

    await act(async () => {
      root.render(<CloudBootstrapPanel machineId="cloud-1" initialSnapshot={snapshot} />);
    });

    const readiness = container.querySelector('[aria-label="First console readiness"]');
    expect(readiness?.getAttribute("data-warm-ready")).toBe("true");

    // Claude already has login_tab_id so RuntimeLoginActions mounts immediately on remote mesh tab.
    const claudeCard = container.querySelector('[data-provider-id="claude"]');
    expect(claudeCard?.getAttribute("data-tab-id")).toBe("mesh:peer-cloud-1/tab-claude");

    // ACP profile prepares its sign-in card on owner click and mounts RuntimeLoginActions.
    const prepareButton = Array.from(container.querySelectorAll("button")).find((btn) =>
      btn.textContent?.includes("ACP (antigravity)"),
    );
    expect(prepareButton).toBeTruthy();
    await act(async () => {
      prepareButton!.click();
    });
    expect(prepareMock).toHaveBeenCalledWith("cloud-1", {
      provider_id: "acp",
      profile_id: "antigravity",
    });
    const agCard = container.querySelector('[data-provider-id="acp"]');
    expect(agCard?.getAttribute("data-tab-id")).toBe("mesh:peer-cloud-1/tab-ag");

    // Ready and unsupported providers retain truthful states.
    const readyList = container.querySelector('[aria-label="Ready providers"]');
    expect(readyList?.textContent).toContain("Codex");
    const unsupportedList = container.querySelector('[aria-label="Unsupported providers"]');
    expect(unsupportedList?.textContent).toContain("Custom CLI");
  });

  it("refuses ready state when readable_owner_context is false", async () => {
    const snapshot: CloudBootstrapSnapshot = {
      machine_id: "cloud-2",
      name: "unverified-box",
      state: "awake",
      peer_id: "peer-cloud-2",
      warm_bootstrap: {
        schema_version: 1,
        status: "failed",
        reject_reason: "readback_unverified",
        creation_generation: "cloud-2:req-1",
        readable_owner_context: false,
        wake_up_verified: false,
        applied_counts: {
          identity_fields: 0,
          wake_up_items: 0,
          rules: 0,
          skills: 0,
          preference_fields: 0,
        },
        excluded: [],
        warnings: [],
        providers: [],
      },
    };

    await act(async () => {
      root.render(<CloudBootstrapPanel machineId="cloud-2" initialSnapshot={snapshot} />);
    });

    const readiness = container.querySelector('[aria-label="First console readiness"]');
    expect(readiness?.getAttribute("data-warm-ready")).toBe("false");
  });

  it("merges live runtime login replies into the detached remote sign-in card for an opaque provider", async () => {
    const snapshot: CloudBootstrapSnapshot = {
      machine_id: "cloud-3",
      name: "opaque-box",
      state: "awake",
      peer_id: "peer-cloud-3",
      warm_bootstrap: {
        schema_version: 1,
        status: "applied",
        creation_generation: "cloud-3:req-1",
        readable_owner_context: true,
        wake_up_verified: true,
        applied_counts: {
          identity_fields: 1,
          wake_up_items: 1,
          rules: 0,
          skills: 0,
          preference_fields: 1,
        },
        excluded: [],
        warnings: [],
        providers: [
          {
            provider_id: "opaque-provider",
            display_name: "Opaque Provider",
            state: "sign_in_required",
            supported_methods: ["browser", "device"],
            code_input: false,
            requires_owner_input: true,
          },
        ],
      },
    };
    prepareMock.mockResolvedValueOnce({
      providers: [
        {
          provider_id: "opaque-provider",
          display_name: "Opaque Provider",
          state: "sign_in_required",
          supported_methods: ["browser", "device"],
          code_input: false,
          tab_id: "tab-opaque",
          remote_tab_id: "mesh:peer-cloud-3/tab-opaque",
          card_ready: true,
          requires_owner_input: true,
        },
      ],
    });

    await act(async () => {
      root.render(<CloudBootstrapPanel machineId="cloud-3" initialSnapshot={snapshot} />);
    });

    const signInGroup = container.querySelector('[role="group"][aria-label="Sign in to Opaque Provider"]');
    expect(signInGroup).not.toBeNull();
    const prepareButton = signInGroup?.querySelector<HTMLButtonElement>("button");
    expect(prepareButton?.textContent).toBe("Sign in to Opaque Provider");

    await act(async () => {
      prepareButton!.click();
    });

    const card = container.querySelector('[data-provider-id="opaque-provider"]');
    expect(card?.getAttribute("data-tab-id")).toBe("mesh:peer-cloud-3/tab-opaque");
    expect(card?.getAttribute("data-user-code")).toBe("");

    const mintBtn = container.querySelector<HTMLButtonElement>('[data-testid="mock-reply-opaque-provider"]');
    await act(async () => {
      mintBtn!.click();
    });

    expect(card?.getAttribute("data-user-code")).toBe("OPAQ-7741");
    expect(card?.getAttribute("data-binding-nonce")).toBe("opaque-nonce-1");
  });
});
