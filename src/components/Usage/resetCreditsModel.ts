import type {ResetCredits, ResetCreditOutcome} from "@codeterm/plugin-sdk";

export interface ResetResponse {outcome: ResetCreditOutcome}
export class ResetCreditsModel {
  confirming = false;
  busy = false;
  key: string | null = null;
  outcome: ResetCreditOutcome | null = null;
  error: string | null = null;

  constructor(private uuid: () => string, private consume: (key: string) => Promise<ResetResponse>) {}

  request(resets: ResetCredits | null | undefined): void {
    if (!resets || resets.availableCount === 0 || this.busy || this.key) return;
    this.confirming = true;
  }

  cancel(): void { this.confirming = false; }

  async confirm(): Promise<void> {
    if (!this.confirming || this.busy) return;
    this.confirming = false;
    this.key = this.uuid();
    await this.retry();
  }

  async retry(): Promise<void> {
    if (!this.key || this.busy) return;
    this.busy = true;
    this.error = null;
    this.outcome = null;
    try {
      const response = await this.consume(this.key);
      this.outcome = response.outcome;
      this.key = null;
    } catch {
      this.error = "Reset request could not be confirmed. Retry safely with the same request.";
    } finally { this.busy = false; }
  }
}

export function resetCreditsState(resets: ResetCredits | null | undefined) {
  return {visible: !!resets, disabled: !resets || resets.availableCount === 0};
}

export const resetOutcomeLabels: Record<ResetCreditOutcome, string> = {
  reset: "Reset done", nothingToReset: "Nothing to reset", noCredit: "No credit available", alreadyRedeemed: "Already redeemed",
};
