import type {ResetCredits, ResetCreditOutcome, ResetCredit} from "@codeterm/plugin-sdk";

export interface ResetResponse {outcome: ResetCreditOutcome; remainingCount?: number|null}
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

export function resetCreditsForTab(tabId: string, uuid: () => string, consume: (key: string, creditId?: string) => Promise<ResetResponse>): ResetCreditsModel {
  let entry = tabResets.get(tabId);
  if (!entry) {
    entry = new ResetCreditsModel(uuid, consume);
    tabResets.set(tabId, entry);
  }
  return entry;
}

export function clearResetCreditsForTab(tabId: string): void {
  tabResets.get(tabId)?.signOut();
  tabResets.delete(tabId);
}
