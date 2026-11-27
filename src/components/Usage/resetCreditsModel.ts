import type {ResetCredits, ResetCreditOutcome, ResetCredit} from "@codeterm/plugin-sdk";
import type {ProviderDescriptor} from "../../lib/types";
import type {RuntimeUsageProjection} from "../../types/usage";

export interface ResetResponse {outcome: ResetCreditOutcome; remainingCount?: number|null}
export interface ResetCreditsTarget {
  tabId: string | null;
  threadId: string | null;
  readOnlyReason: string | null;
}

export class ResetCreditsModel {
  selectedCreditId: string | null = null;
  pendingCredit: ResetCredit | null = null;
  confirming = false;
  busy = false;
  key: string | null = null;
  outcome: ResetCreditOutcome | null = null;
  error: string | null = null;
  beforeCount: number | null = null;
  afterCount: number | null = null;

  inFlight: Promise<void> | null = null;
  private revision = 0;
  private generation = 0;
  private listeners = new Set<() => void>();
  subscribe = (listener: () => void) => {this.listeners.add(listener); return () => {this.listeners.delete(listener);};};
  snapshot = () => this.revision;
  private changed() {this.revision++; for (const listener of this.listeners) listener();}

  constructor(private uuid: () => string, private consume: (key: string, creditId?: string) => Promise<ResetResponse>) {}

  setConsume(consume: (key: string, creditId?: string) => Promise<ResetResponse>): void {
    if (!this.inFlight) this.consume = consume;
  }

  select(resets: ResetCredits, creditId: string | null): void {
    if (this.busy || this.key || this.confirming || resetCreditsState(resets, creditId).disabled) return;
    this.selectedCreditId = creditId;
    this.changed();
  }

  request(resets: ResetCredits | null | undefined): void {
    if (!resets || resetCreditsState(resets, this.selectedCreditId).disabled || this.busy || this.key) return;
    this.pendingCredit = availableResetCredits(resets).find(credit => credit.id === this.selectedCreditId) ?? null;
    this.confirming = true;
    this.beforeCount = resets.availableCount;
    this.afterCount = null;
    this.outcome = null;
    this.changed();
  }

  cancel(): void { this.confirming = false; this.changed(); }

  signOut(): void {
    this.generation++;
    this.selectedCreditId = null;
    this.pendingCredit = null;
    this.confirming = false;
    this.key = null;
    this.busy = false;
    this.inFlight = null;
    this.outcome = null;
    this.error = null;
    this.beforeCount = null;
    this.afterCount = null;
    this.changed();
  }

  async confirm(): Promise<void> {
    if (!this.confirming || this.busy) return;
    this.confirming = false;
    this.key = this.uuid();
    await this.retry();
  }

  retry(): Promise<void> {
    if (this.inFlight) return this.inFlight;
    if (!this.key) return Promise.resolve();
    this.busy = true;
    this.error = null;
    this.outcome = null;
    const generation = this.generation;
    const key = this.key;
    let complete!: () => void;
    const pending = new Promise<void>(resolve => {complete=resolve;});
    this.inFlight = pending;
    this.changed();
    void this.consumePending(key, generation).then(complete);
    return pending;
  }

  private async consumePending(key: string, generation: number): Promise<void> {
    try {
      const response = await this.consume(key, this.pendingCredit?.id);
      if (generation !== this.generation) return;
      if (!Object.prototype.hasOwnProperty.call(resetOutcomeLabels, response.outcome)) throw new Error("Unknown reset outcome");
      this.outcome = response.outcome;
      this.afterCount = typeof response.remainingCount === "number" && Number.isSafeInteger(response.remainingCount) && response.remainingCount >= 0 ? response.remainingCount : null;
      this.key = null;
    } catch {
      if (generation === this.generation) this.error = "Reset request could not be confirmed. Retry safely with the same request.";
    } finally {
      if (generation === this.generation) {
        this.busy = false;
        this.inFlight = null;
        this.changed();
      }
    }
  }
}

export function resetBalanceLabel(before: number|null, after: number|null): string {
  return `Resets: ${before ?? "unknown"} before · ${after ?? "unconfirmed"} after`;
}

export function availableResetCredits(resets: ResetCredits | null | undefined): ResetCredit[] {
  const seen = new Set<string>();
  return (resets?.credits ?? []).filter(credit => {
    if (credit.status !== "available" || !credit.id || seen.has(credit.id)) return false;
    seen.add(credit.id);
    return true;
  });
}

export function resetCreditLabel(credit: ResetCredit): string {
  return credit.title || "Session reset";
}

export function resetCreditsState(resets: ResetCredits | null | undefined, selectedId: string | null = null) {
  return {visible: !!resets, disabled: !resets || resets.availableCount === 0
    || (selectedId !== null && !availableResetCredits(resets).some(credit => credit.id === selectedId))};
}

export const resetOutcomeLabels: Record<ResetCreditOutcome, string> = {
  reset: "Reset done", nothingToReset: "Nothing to reset", noCredit: "No credit available", alreadyRedeemed: "Already redeemed",
};

const tabResets = new Map<string, ResetCreditsModel>();

export function resetCountSummary(count: number): string {
  return `${count} ${count === 1 ? "reset" : "resets"} available · the provider picks which one is used`;
}

export function providerSupportsResetCredits(descriptor: Pick<ProviderDescriptor, "agent_runtime"> | undefined): boolean {
  return descriptor ? Boolean(descriptor.agent_runtime?.resetCreditsMinVersion) : true;
}

export function resolveResetCreditsTarget(params: {
  tabId?: string;
  threadId?: string | null;
  peerId?: string | null;
  borrowedAccount: boolean;
  accountReading: RuntimeUsageProjection | null;
  providerSupportsResets: boolean;
}): ResetCreditsTarget {
  if (params.peerId) {
    return {tabId: null, threadId: null, readOnlyReason: "Use resets on the machine running this account."};
  }
  if (!params.providerSupportsResets) {
    return {tabId: null, threadId: null, readOnlyReason: "This provider doesn’t support redeeming resets here."};
  }
  if (params.borrowedAccount) {
    const ownerTabId = params.accountReading?.tab_id || null;
    if (!ownerTabId) {
      return {tabId: null, threadId: null, readOnlyReason: "Open a tab for this account to use resets."};
    }
    return {tabId: ownerTabId, threadId: params.accountReading?.thread_id ?? null, readOnlyReason: null};
  }
  if (!params.tabId) {
    return {tabId: null, threadId: null, readOnlyReason: "Resets aren’t available from this view."};
  }
  return {tabId: params.tabId, threadId: params.threadId ?? null, readOnlyReason: null};
}

export function resetCreditsForTab(tabId: string, uuid: () => string, consume: (key: string, creditId?: string) => Promise<ResetResponse>): ResetCreditsModel {
  let entry = tabResets.get(tabId);
  if (!entry) {
    entry = new ResetCreditsModel(uuid, consume);
    tabResets.set(tabId, entry);
  } else {
    entry.setConsume(consume);
  }
  return entry;
}

export function clearResetCreditsForTab(tabId: string): void {
  tabResets.get(tabId)?.signOut();
  tabResets.delete(tabId);
}
