import type {ResetCredits, ResetCreditOutcome} from "@codeterm/plugin-sdk";

export interface ResetResponse {outcome: ResetCreditOutcome}
export class ResetCreditsModel {
  confirming = false;
  busy = false;
  key: string | null = null;
  outcome: ResetCreditOutcome | null = null;
  error: string | null = null;

  inFlight: Promise<void> | null = null;
  private revision = 0;
  private generation = 0;
  private listeners = new Set<() => void>();
  subscribe = (listener: () => void) => {this.listeners.add(listener); return () => {this.listeners.delete(listener);};};
  snapshot = () => this.revision;
  private changed() {this.revision++; for (const listener of this.listeners) listener();}

  constructor(private uuid: () => string, private consume: (key: string) => Promise<ResetResponse>) {}

  request(resets: ResetCredits | null | undefined): void {
    if (!resets || resets.availableCount === 0 || this.busy || this.key) return;
    this.confirming = true;
    this.changed();
  }

  cancel(): void { this.confirming = false; this.changed(); }

  signOut(): void {
    this.generation++;
    this.confirming = false;
    this.key = null;
    this.busy = false;
    this.inFlight = null;
    this.outcome = null;
    this.error = null;
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
      const response = await this.consume(key);
      if (generation !== this.generation) return;
      if (!Object.prototype.hasOwnProperty.call(resetOutcomeLabels, response.outcome)) throw new Error("Unknown reset outcome");
      this.outcome = response.outcome;
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

export function resetCreditsState(resets: ResetCredits | null | undefined) {
  return {visible: !!resets, disabled: !resets || resets.availableCount === 0};
}

export const resetOutcomeLabels: Record<ResetCreditOutcome, string> = {
  reset: "Reset done", nothingToReset: "Nothing to reset", noCredit: "No credit available", alreadyRedeemed: "Already redeemed",
};

const tabResets = new Map<string, ResetCreditsModel>();

export function resetCreditsForTab(tabId: string, uuid: () => string, consume: (key: string) => Promise<ResetResponse>): ResetCreditsModel {
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
