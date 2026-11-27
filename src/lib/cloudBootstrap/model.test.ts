import { describe, expect, it } from "vitest";
import {
  buildRemoteRuntimeLoginState,
  isWarmConsoleReady,
  partitionProviderCards,
  resolveRemoteLoginTabId,
  summarizeCreatePhases,
} from "./model";
import type { CloudBootstrapSnapshot, ProviderSignInCardStatus } from "./types";

function makeSnapshot(
  overrides: Partial<NonNullable<CloudBootstrapSnapshot["warm_bootstrap"]>>,
): CloudBootstrapSnapshot {
  return {
    machine_id: "cloud-1",
    name: "warm-box",
    state: "awake",
    peer_id: "peer-cloud-1",
    warm_bootstrap: {
      schema_version: 1,
      status: "applied",
      creation_generation: "cloud-1:req-1",
      readable_owner_context: true,
      wake_up_verified: true,
      applied_counts: {
        identity_fields: 1,
        wake_up_items: 2,
        rules: 1,
        skills: 1,
        preference_fields: 3,
      },
      excluded: [],
      warnings: [],
      providers: [],
      ...overrides,
    },
  };
}

describe("cloudBootstrap pure model", () => {
  it("never reports ready without readable verified owner context", () => {
    expect(isWarmConsoleReady(makeSnapshot({ readable_owner_context: false }))).toBe(false);
    expect(isWarmConsoleReady(makeSnapshot({ wake_up_verified: false }))).toBe(false);
    expect(isWarmConsoleReady(makeSnapshot({ status: "failed" }))).toBe(false);
    expect(isWarmConsoleReady(makeSnapshot({ status: "rejected" }))).toBe(false);
    expect(isWarmConsoleReady(makeSnapshot({ status: "applied" }))).toBe(true);
    expect(isWarmConsoleReady(makeSnapshot({ status: "partial" }))).toBe(true);
    expect(isWarmConsoleReady(makeSnapshot({ status: "skipped_restored_state" }))).toBe(true);
  });

  it("partitions signed-out supported, ready, and unsupported providers truthfully", () => {
    const providers: ProviderSignInCardStatus[] = [
      {
        provider_id: "claude",
        display_name: "Claude",
        state: "sign_in_required",
        supported_methods: ["browser"],
        code_input: true,
        login_tab_id: "tab-c",
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
        provider_id: "acp",
        profile_id: "antigravity",
        display_name: "ACP (antigravity)",
        state: "sign_in_required",
        supported_methods: ["browser", "apiKey"],
        code_input: true,
        requires_owner_input: true,
      },
      {
        provider_id: "legacy-cli",
        display_name: "Legacy CLI",
        state: "unsupported",
        supported_methods: [],
        code_input: false,
        requires_owner_input: false,
        unsupported_reason: "login_flow_unsupported",
      },
    ];
    const grouped = partitionProviderCards(providers);
    expect(grouped.signInRequired.map((p) => p.provider_id)).toEqual(["claude", "acp"]);
    expect(grouped.ready.map((p) => p.provider_id)).toEqual(["codex"]);
    expect(grouped.unsupported.map((p) => p.provider_id)).toEqual(["legacy-cli"]);

    const loginState = buildRemoteRuntimeLoginState(providers[0]);
    expect(loginState.auth).toBe("signedOut");
    expect(loginState.loginCodeInput).toBe(true);
    expect(loginState.loginMethods).toEqual(["browser"]);
    expect(resolveRemoteLoginTabId("peer-1", providers[0])).toBe("mesh:peer-1/tab-c");
  });

  it("measures peer_ready, backend-authoritative warm bootstrap delta, and human sign-in separately", () => {
    const summary = summarizeCreatePhases({
      requested_at_ms: 1_000,
      peer_ready_at_ms: 16_000,
      seed_started_at_ms: 16_000,
      warm_console_ready_at_ms: 19_000,
      warm_bootstrap_delta_secs: 3.0,
      authoritative_backend_phases: true,
      provider_signin_started_at_ms: 25_000,
      provider_signin_completed_at_ms: 70_000,
    });
    expect(summary.peerReadyDeltaSecs).toBe(15);
    expect(summary.warmBootstrapDeltaSecs).toBe(3.0);
    expect(summary.withinBudget).toBe(true);
    expect(summary.providerSignInSecs).toBe(45);

    // Client-only timestamps without backend authority must not fabricate warm_bootstrap_delta_secs.
    const unverified = summarizeCreatePhases({
      requested_at_ms: 1_000,
      peer_ready_at_ms: 16_000,
      seed_started_at_ms: 16_000,
      warm_console_ready_at_ms: 19_000,
      authoritative_backend_phases: false,
    });
    expect(unverified.warmBootstrapDeltaSecs).toBeNull();
    expect(unverified.withinBudget).toBeNull();
  });
});
