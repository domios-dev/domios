export interface ComposerAttachmentPlan {
  path: string;
  transcribe: boolean;
}

export function planComposerAttachments(
  attachments: string[],
  recordedVoiceAttachments: string[],
): ComposerAttachmentPlan[] {
  const recordedVoice = new Set(recordedVoiceAttachments);
  return attachments.map((path) => ({ path, transcribe: recordedVoice.has(path) }));
}

// A native async clipboard read pops the WebView's own Paste button, so only an
// image-less paste that is not plain text falls back to it (copied images carry HTML).
export function needsAsyncClipboardImageRead(eventTypes: readonly string[], hasEventImage: boolean): boolean {
  return !hasEventImage && !(eventTypes.includes("text/plain") && !eventTypes.includes("text/html"));
}
