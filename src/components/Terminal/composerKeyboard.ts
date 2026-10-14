export type ComposerDevice = "desktop" | "ipad" | "phone";

export interface ComposerDeviceSignals {
  userAgent: string;
  platform: string;
  maxTouchPoints: number;
  touchOptimizedUi: boolean;
}

export function classifyComposerDevice(signals: ComposerDeviceSignals): ComposerDevice {
  if (/iPad/i.test(signals.userAgent) || /iPad/i.test(signals.platform)
    || ((/^Mac/i.test(signals.platform) || /Macintosh/i.test(signals.userAgent)) && signals.maxTouchPoints > 1)) return "ipad";
  if (/iPhone|iPod/i.test(signals.userAgent) || /iPhone|iPod/i.test(signals.platform)
    || signals.touchOptimizedUi) return "phone";
  return "desktop";
}

export interface ComposerKey {
  code: string;
  key?: string;
  keyCode?: number;
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  isComposing: boolean;
}

export function isComposerEnterKey(key: Pick<ComposerKey, "code" | "key">): boolean {
  return key.code === "Enter" || key.code === "NumpadEnter"
    || ((key.code === "" || key.code === "Unidentified") && key.key === "Enter");
}

export function composerEnterIntent(
  device: ComposerDevice,
  key: ComposerKey,
): "submit" | "newline" | "none" {
  if (key.isComposing || key.keyCode === 229 || !isComposerEnterKey(key)) return "none";
  if (key.ctrlKey || key.metaKey || key.altKey) return "none";
  return key.shiftKey || device === "phone" ? "newline" : "submit";
}
