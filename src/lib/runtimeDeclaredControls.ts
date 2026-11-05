import type { RuntimeConfigOption, RuntimeSettings } from "@codeterm/plugin-sdk";
import { invoke } from "./bridge";

/**
 * A runtime agent (ACP and the like) declares its own controls. The model and the reasoning effort belong to the
 * status-line model picker, so a tab shows one model control; every other declared control stays in the
 * runtime controls row. The match is only by the category the agent declares (ACP: "model", "thought_level"),
 * which the provider plugin carries through; never by option id, name or provider, so an undeclared option
 * is never guessed to be a model.
 */
export type PickerControl = "model" | "effort";

const CATEGORY: Record<string, PickerControl> = { model: "model", thought_level: "effort" };

export function pickerControlOf(option: RuntimeConfigOption): PickerControl | undefined {
  return option.category ? CATEGORY[option.category] : undefined;
}

/** The declared options the model picker owns; only a choice with values can be owned. */
export function pickerControls(controls: RuntimeSettings | undefined): { model?: RuntimeConfigOption; effort?: RuntimeConfigOption } {
  const owned: { model?: RuntimeConfigOption; effort?: RuntimeConfigOption } = {};
  for (const option of controls?.configOptions ?? []) {
    const kind = pickerControlOf(option);
    if (kind && !owned[kind] && option.options.length) owned[kind] = option;
  }
  return owned;
}

/** The declared options left for the runtime controls row. */
export function rowControls(controls: RuntimeSettings | undefined): readonly RuntimeConfigOption[] {
  const owned = pickerControls(controls);
  return (controls?.configOptions ?? []).filter((option) => option !== owned.model && option !== owned.effort);
}

export interface RuntimeControlTarget {
  threadId?: string | null;
  bindingNonce?: string | null;
  epoch?: number;
}

/** Sets one declared control (or the session mode) through the runtime controls API. */
export function selectRuntimeControl(tabId: string, target: RuntimeControlTarget, id: string, value: string, mode = false): Promise<unknown> {
  return invoke("agent_runtime_controls", { tabId, request: { threadId: target.threadId, bindingNonce: target.bindingNonce, epoch: target.epoch, id, value, mode } });
}
