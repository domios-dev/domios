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
