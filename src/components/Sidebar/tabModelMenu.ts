/** Sidebar-row model switch: which tabs offer it, what the entry says, and the
 *  request it makes. Kept out of TabItem so the gate and the round-trip are
 *  testable without a menu. */
import { supportsLiveModelSwitch } from "../../lib/modelSwitch";
import type { ModelInfo, ProviderDescriptor, Tab, TabStatus } from "../../lib/types";
import { getTabModelApi } from "./tabModelApi";

export interface TabModelMenuView {
  /** Provider-published catalog, in descriptor order. */
  models: ModelInfo[];
  /** The published model this session is currently on, when it can be matched. */
  currentModel: ModelInfo | null;
  tabId: string;
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
 * A route exists only for a live PTY agent tab whose provider declares a live
 * `model_switch` mechanism and a non-empty `available_models`. chatBackend
 * shells are excluded: they own `ChatModelBubble`, which switches through
 * `chatbackend_switch_model` instead. The protected General Agent takes its model
 * from configuration, so its row offers none.
 */
export function tabModelMenuView(
  tab: Tab,
  descriptor: ProviderDescriptor | null | undefined,
  status: TabStatus | null | undefined,
): TabModelMenuView | null {
  if (!tab.agent_type || tab.backing?.kind === "chat_backend" || tab.protected) return null;
  if (!descriptor || descriptor.id !== tab.agent_type) return null;
  // Only an explicit "none" opts out; an absent behavior_config is not a refusal.
  if (descriptor.behavior_config?.model === "none") return null;
  if (!supportsLiveModelSwitch(descriptor)) return null;
  const models = descriptor.available_models ?? [];
  if (models.length === 0) return null;
  if (!tab.pty_id) return null;
  return {
    models,
    currentModel: matchModel(models, tab.agent_session_model),
    tabId: tab.id,
    blockedReason: status === "Working" ? "Working — switch the model once this turn ends" : null,
  };
}

/** Accessible name of the entry that opens the picker, derived from the
 *  descriptor so a renamed model renames the control. */
export function tabModelEntryLabel(view: TabModelMenuView): string {
  return view.currentModel ? `Model: ${view.currentModel.display_name}` : "Model";
}

export async function switchTabModel(view: TabModelMenuView, modelId: string): Promise<void> {
  await getTabModelApi().switchModel(view.tabId, modelId);
}
