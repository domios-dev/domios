/**
 * Pins every head of an agent picker to a stub-replayed provider before the
 * picker is confirmed. A missing or partial stub throws naming the provider:
 * it never skips and never lets a confirm reach a live provider.
 */
import type { ApiClient } from "./api-client";
import { stubEnvActive } from "./perf-seed";
import { isStubTransportActive } from "./stub-gate";

export const STUB_PIN_PROVIDER = "claude";

type ProviderListing = {
  id: string;
  display_name?: string;
  transport?: string;
  agent_runtime?: unknown;
  stub_replay?: boolean;
};

declare const proven: unique symbol;
/** Only `requireStubbedProvider` mints one, so a pin cannot run on an unproven provider. */
export type StubbedProvider = { id: string; display_name: string; readonly [proven]: true };

export type StubPinDeps = {
  env: NodeJS.ProcessEnv;
  transportActive: (api: ApiClient) => Promise<boolean>;
};

const liveDeps: StubPinDeps = { env: process.env, transportActive: isStubTransportActive };

const PIN_TIMEOUT_MS = 15_000;

/** Launch-free proof that confirming `providerId` replays the stub; throws otherwise. */
export async function requireStubbedProvider(
  api: Pick<ApiClient, "get">,
  providerId: string,
  deps: StubPinDeps = liveDeps,
): Promise<StubbedProvider> {
  if (!stubEnvActive(deps.env)) {
    throw new Error(`${providerId} is unstubbed: CODETERM_STUB_AGENT_CMD is not set; refusing a live launch`);
  }
  const only = (deps.env.CODETERM_STUB_AGENT_PROVIDERS ?? "").trim();
  if (only && !only.split(",").some((id) => id.trim().toLowerCase() === providerId.toLowerCase())) {
    throw new Error(`CODETERM_STUB_AGENT_PROVIDERS="${only}" leaves ${providerId} unstubbed; it would launch live`);
  }
  const listing = (await api.get<ProviderListing[]>("/agents/providers")).find((p) => p.id === providerId);
  if (!listing) throw new Error(`/agents/providers does not list the stub provider ${providerId}`);
  if (listing.stub_replay !== true) {
    throw new Error(`daemon does not replay ${providerId} (stub_replay=${JSON.stringify(listing.stub_replay)}); it would launch live`);
  }
  // The replayer wraps PTY launches only; a runtime-capable provider bypasses it.
  if (listing.transport !== "pty" || listing.agent_runtime !== undefined) {
    throw new Error(
      `${providerId} would launch outside the PTY replayer (transport=${listing.transport}, agent_runtime=${JSON.stringify(listing.agent_runtime)})`,
    );
  }
  if (!(await deps.transportActive(api as ApiClient))) {
    throw new Error(`stub transport is not active, so ${providerId} would launch live`);
  }
  return { id: providerId, display_name: listing.display_name ?? providerId } as StubbedProvider;
}

export type PinLocator = {
  click(options?: { timeout?: number }): Promise<void>;
  and(other: PinLocator): PinLocator;
  locator(selector: string): PinLocator;
  waitFor(options: { state: "attached" | "visible"; timeout?: number }): Promise<void>;
};

export type PinPage = {
  locator(selector: string): PinLocator;
  getByRole(role: "menuitem", options: { name: string; exact: true }): PinLocator;
};

/** A dropdown head shows its choice as `data-value`; a card head marks itself `data-selected="yes"`. */
export type PickerHead = { kind: "menu"; trigger: PinLocator } | { kind: "cards"; scope: PinLocator };

async function expectState(target: PinLocator, failure: string): Promise<void> {
  try {
    await target.waitFor({ state: "attached", timeout: PIN_TIMEOUT_MS });
  } catch (cause) {
    throw new Error(failure, { cause });
  }
}

async function pinHead(page: PinPage, provider: StubbedProvider, head: PickerHead): Promise<void> {
  if (head.kind === "cards") {
    const card = head.scope.locator(`[data-testid="agent-picker-card-${provider.id}"]`);
    await card.click({ timeout: PIN_TIMEOUT_MS });
    await expectState(card.and(page.locator('[data-selected="yes"]')), `card head must launch the stubbed ${provider.id}`);
    return;
  }
  await expectState(
    head.trigger.and(page.locator(':not([data-value=""])')),
    "provider catalogue must load before a head can be pinned",
  );
  await head.trigger.click();
  try {
    await page.getByRole("menuitem", { name: provider.display_name, exact: true }).click({ timeout: PIN_TIMEOUT_MS });
  } catch (cause) {
    throw new Error(`${provider.id} must be installed and selectable`, { cause });
  }
  await expectState(
    head.trigger.and(page.locator(`[data-value="${provider.id}"]`)),
    `head must launch the stubbed ${provider.id}`,
  );
}

/** Selects `provider` on every head through the picker's own controls and asserts each. */
export async function pinHeadsToStub(page: PinPage, provider: StubbedProvider, heads: PickerHead[]): Promise<void> {
  if (heads.length === 0) throw new Error(`no picker head to pin to ${provider.id}`);
  for (const head of heads) await pinHead(page, provider, head);
}

/** Requires the stub for `providerId`, then pins every head to it. */
export async function pinPickerToStub(
  api: Pick<ApiClient, "get">,
  page: PinPage,
  heads: PickerHead[],
  providerId: string = STUB_PIN_PROVIDER,
  deps: StubPinDeps = liveDeps,
): Promise<StubbedProvider> {
  const provider = await requireStubbedProvider(api, providerId, deps);
  await pinHeadsToStub(page, provider, heads);
  return provider;
}
