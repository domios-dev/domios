export type SettingValue = boolean | number | string;

export interface SettingOption {
  value: string;
  label: string;
}

/** The control a schema field maps to: boolean, enum, number or short string. */
export type SettingControl =
  | { kind: "switch" }
  | { kind: "choice"; options: SettingOption[] }
  | { kind: "stepper"; min?: number; max?: number; step: number }
  | { kind: "text"; placeholder?: string };

/** One config value plus the label, description and control its settings schema declares. */
export interface SettingInfo {
  key: string;
  label: string;
  description?: string;
  /** Who owns the setting, e.g. the plugin's display name. */
  owner?: string;
  control: SettingControl;
  value: SettingValue;
}

export type SettingRefusal = "unknown" | "sensitive";

export type SettingBlockState =
  | { kind: "loading" }
  | { kind: "unavailable"; message: string }
  | { kind: "refused"; reason: SettingRefusal }
  | { kind: "ready"; setting: SettingInfo };

const SEGMENT = "[A-Za-z0-9_][A-Za-z0-9_-]{0,63}";
const CONFIG_KEY = new RegExp(`^${SEGMENT}(?:\\.${SEGMENT}){1,7}$`);

export function parseSettingKey(argument: string): string | null {
  const key = argument.trim();
  return CONFIG_KEY.test(key) ? key : null;
}

const SENSITIVE_SEGMENT = /token|secret|passw(?:or)?d|credential|api[_-]?key|private[_-]?key|auth|cookie|session[_-]?key/i;

/** Secrets never render inline, whatever the schema says about them. */
export function isSensitiveKey(key: string): boolean {
  return key.split(".").some((segment) => SENSITIVE_SEGMENT.test(segment));
}

const SEGMENTED_MAX_OPTIONS = 4;
const SEGMENTED_MAX_LABEL = 14;

/** A few short options read best side by side; anything longer is a select. */
export function choiceLayout(options: readonly SettingOption[]): "segmented" | "select" {
  return options.length <= SEGMENTED_MAX_OPTIONS && options.every((option) => option.label.length <= SEGMENTED_MAX_LABEL) ? "segmented" : "select";
}

export function stepValue(value: number, direction: 1 | -1, control: Extract<SettingControl, { kind: "stepper" }>): number {
  const next = Number((value + direction * control.step).toFixed(6));
  if (control.min !== undefined && next < control.min) return control.min;
  if (control.max !== undefined && next > control.max) return control.max;
  return next;
}

export function canStep(value: number, direction: 1 | -1, control: Extract<SettingControl, { kind: "stepper" }>): boolean {
  return stepValue(value, direction, control) !== value;
}

export const SETTING_REFUSAL_TEXT: Record<SettingRefusal, string> = {
  unknown: "This setting is not available here.",
  sensitive: "This setting is not available here: secrets and tokens stay in Settings.",
};
