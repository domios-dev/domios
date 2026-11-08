import type { RuntimeConfigOption, RuntimeSettings } from "@codeterm/plugin-sdk";
import { invoke } from "./bridge";

export type PickerControl = "model" | "effort";

const CATEGORY: Record<string, PickerControl> = { model: "model", thought_level: "effort" };

export function pickerControlOf(option: RuntimeConfigOption): PickerControl | undefined {
  return option.category ? CATEGORY[option.category] : undefined;
}

export function pickerControls(controls: RuntimeSettings | undefined): { model?: RuntimeConfigOption; effort?: RuntimeConfigOption } {
  const owned: { model?: RuntimeConfigOption; effort?: RuntimeConfigOption } = {};
  for (const option of controls?.configOptions ?? []) {
    const kind = pickerControlOf(option);
    if (kind && !owned[kind] && option.options.length) owned[kind] = option;
  }
  return owned;
}

export interface RuntimeControlTarget {
  threadId?: string | null;
  bindingNonce?: string | null;
  epoch?: number;
}

/** The owner's live answer to this tab's permission requests; applies without a restart. */
export function setHostApprovals(tabId: string, allowAll: boolean): Promise<unknown> {
  return invoke("agent_runtime_host_approvals", { tabId, request: { allowAll } });
}

/** Sets one declared control (or the session mode) through the runtime controls API. */
export function selectRuntimeControl(tabId: string, target: RuntimeControlTarget, id: string, value: string, mode = false): Promise<unknown> {
  return invoke("agent_runtime_controls", { tabId, request: { threadId: target.threadId, bindingNonce: target.bindingNonce, epoch: target.epoch, id, value, mode } });
}
