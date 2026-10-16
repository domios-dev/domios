export type ResyncReason = "overflow" | "epoch_changed" | "reconnected" | "wake";

export function resyncRequiresTranscriptRecovery(reason: ResyncReason, attachmentFailed = false): boolean {
  return attachmentFailed || reason === "overflow" || reason === "epoch_changed";
}

export function replayCursorForTarget(
  currentTarget: string | null,
  currentCursor: string | null,
  nextTarget: string,
  storedCursor: string | null,
): string | null {
  return currentTarget === nextTarget ? currentCursor ?? storedCursor : storedCursor;
}
