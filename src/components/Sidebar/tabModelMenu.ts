/** Sidebar-row model switch: which tabs offer it, what the entry says, and the
 *  payload it writes. Kept out of TabItem so the gate and the round-trip are
 *  testable without a menu. */
import { formatModelSwitch } from "../../lib/modelSwitch";
import type { ModelInfo, ProviderDescriptor, Tab, TabStatus } from "../../lib/types";
import { getTabModelApi } from "./tabModelApi";

export interface TabModelMenuView {
  /** Provider-published catalog, in descriptor order. */
  models: ModelInfo[];
  /** The published model this session is currently on, when it can be matched. */
  currentModel: ModelInfo | null;
  switchFormat: string;
  ptyId: string;
  /** Non-null while a turn is in flight — the entry stays inert with this reason. */
  blockedReason: string | null;
}

/** Descriptor `available_models` ids are provider vocabulary; a detected session
 *  model can be a normalised family of the same id (Claude reports "Opus" for
 *  "opus"), so match exactly first and case-insensitively second. */
function matchModel(models: ModelInfo[], sessionModel: string | null | undefined): ModelInfo | null {
  if (!sessionModel) return null;
  const exact = models.find((model) => model.id === sessionModel);
  if (exact) return exact;
  const folded = sessionModel.toLowerCase();
  return models.find((model) => model.id.toLowerCase() === folded) ?? null;
}

/**
 * The row's model-switch surface, or null when this tab has no live switch route.
 *
 * A route exists only for a PTY agent tab whose provider publishes both a
 * `model_switch_format` (the in-agent command) and a non-empty `available_models`.
 * chatBackend shells are excluded: they own `ChatModelBubble`, which switches
 * through `chatbackend_switch_model` instead. `tab.protected` is deliberately not
 * a gate — protection suppresses destroying or renaming the anchor tab, and an
 * in-session model command does neither.
 */
export function tabModelMenuView(
  tab: Tab,
  descriptor: ProviderDescriptor | null | undefined,
  status: TabStatus | null | undefined,
): TabModelMenuView | null {
  if (!tab.agent_type || tab.backing?.kind === "chat_backend") return null;
  if (!descriptor || descriptor.id !== tab.agent_type) return null;
  // Only an explicit "none" opts out; an absent behavior_config is not a refusal.
  if (descriptor.behavior_config?.model === "none") return null;
  const switchFormat = descriptor.model_switch_format?.trim();
  if (!switchFormat) return null;
  const models = descriptor.available_models ?? [];
  if (models.length === 0) return null;
  const ptyId = tab.pty_id;
  if (!ptyId) return null;
  return {
    models,
    currentModel: matchModel(models, tab.agent_session_model),
    switchFormat,
    ptyId,
    blockedReason: status === "Working" ? "Working — the model changes after this turn" : null,
  };
}

/** Accessible name of the entry that opens the picker, derived from the
 *  descriptor so a renamed model renames the control. */
export function tabModelEntryLabel(view: TabModelMenuView): string {
  return view.currentModel ? `Model: ${view.currentModel.display_name}` : "Model";
}

/** The exact bytes handed to the PTY, including the submit key. */
export function tabModelSwitchPayload(view: TabModelMenuView, modelId: string): string {
  return `${formatModelSwitch(view.switchFormat, modelId)}\r`;
}

export async function switchTabModel(view: TabModelMenuView, modelId: string): Promise<void> {
  await getTabModelApi().switchModel(view.ptyId, tabModelSwitchPayload(view, modelId));
}
