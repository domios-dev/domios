export interface RecordingInput {
  inputId: string;
  collectionId: string;
}

export interface RecordingMicrophone {
  deviceId: string;
  label: string;
}

export interface RecordingSelection extends RecordingInput {
  microphoneId: string;
}

export function offeredRecordingSelection(
  value: unknown,
  inputs: readonly RecordingInput[],
  microphones: readonly RecordingMicrophone[],
): RecordingSelection | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.inputId !== "string" || typeof candidate.collectionId !== "string"
    || typeof candidate.microphoneId !== "string") return null;
  const input = inputs.find((item) => item.inputId === candidate.inputId
    && item.collectionId === candidate.collectionId);
  const microphone = microphones.find((item) => item.deviceId === candidate.microphoneId
    && item.deviceId !== "default" && item.deviceId !== "communications");
  return input && microphone ? { ...input, microphoneId: microphone.deviceId } : null;
}
