import { useEffect, useMemo, useState } from "react";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { emitPaneUiMode, isChatDefaultEnabled, type PaneUiMode } from "../../lib/chatUiMode";
import {
  listenCurrentPaneUiMode,
  listenPaneUiModeDefaultChanged,
  resolveInitialPaneUiMode,
} from "../../lib/clientPaneUiMode";
import { listen } from "../../lib/bridge";
import { useMultiplexer } from "../../store/multiplexer";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { canvasFocusStore, usePlanPulseVersion } from "../Canvas/canvasFocusStore";
import { setPanePlanMode, usePanePlanMode } from "../Canvas/panePlanMode";
import { findPlanIndexForPane, resolveOrchestratorPaneId } from "../Canvas/planPaneMatch";
import { usePlanIndexOptional } from "../Canvas/PlansProvider";

type SwitchValue = PaneUiMode | "plan";

const TERMINAL: SegmentedOption<SwitchValue> = { value: "terminal", label: "Terminal" };
const CHAT: SegmentedOption<SwitchValue> = { value: "chat", label: "Chat" };

/** Terminal | Chat | Plan switch for a pane. Terminal/Chat fully swap the body
 *  (TerminalPane mounts/unmounts the chat surface). Plan is a third, transient
 *  view mode (§7.1): selecting it renders the plan canvas over the pane body via
 *  panePlanMode; its label is role-driven (Plan for an orchestrator that owns a
 *  plan, Tasks for a worker bound to one) and it pulses on plan-changed. */
export function PaneModeSwitch({
  paneId,
  height,
  chatFirst = false,
}: {
  paneId: string;
  height?: number;
  chatFirst?: boolean;
}) {
  const { state } = useMultiplexer();
  const pane = state.panes[paneId];
  const descriptor = useAgentDescriptor(pane?.agent_type);
  const chatCapable =
    pane?.backing?.kind === "chat_backend"
      ? true
      : !pane?.agent_type
        ? false
        : descriptor
          ? descriptor.supports_chat_ui === true
          : true;

  const [mode, setMode] = useState<PaneUiMode>(() =>
    resolveInitialPaneUiMode(paneId, {
      chatCapable,
      chatViewFlag: pane?.chat_view === true,
      chatDefault: isChatDefaultEnabled(),
    }),
  );
  const [defaultChatFirst, setDefaultChatFirst] = useState(() => chatFirst || isChatDefaultEnabled());
  useEffect(() => listenCurrentPaneUiMode(paneId, setMode), [paneId]);
  useEffect(() => {
    setDefaultChatFirst(chatFirst || isChatDefaultEnabled());
  }, [chatFirst]);
  useEffect(() => listenPaneUiModeDefaultChanged((enabled) => {
    setDefaultChatFirst(enabled);
    setMode(resolveInitialPaneUiMode(paneId, {
      chatCapable,
      chatViewFlag: pane?.chat_view === true,
      chatDefault: enabled,
    }));
  }), [paneId, chatCapable, pane?.chat_view]);

  // Plan relevance + role-driven label (§7.1 / §7).
  const { index } = usePlanIndexOptional();
  const relevant = useMemo(
    () => findPlanIndexForPane(index, paneId, state.panes),
    [index, paneId, state.panes],
  );
  const planIds = useMemo(() => relevant.map((p) => p.plan_id), [relevant]);
  const orchestratorPaneId = useMemo(
    () => resolveOrchestratorPaneId(paneId, state.panes),
    [paneId, state.panes],
  );
  const ownsPlan = relevant.some(
    (p) => p.owner.pane_id === paneId || p.owner.pane_id === orchestratorPaneId,
  );
  const planActive = usePanePlanMode(paneId);
  usePlanPulseVersion(planIds);
  const pulsing = !planActive && canvasFocusStore.anyPulsing(planIds);

  // Pulse the Plan segment on plan-changed touching this pane's plans.
  useEffect(() => {
    if (planIds.length === 0) return;
    let active = true;
    let unlisten: (() => void) | null = null;
    const ids = new Set(planIds);
    void listen<{ plan_id?: string }>("plan-changed", (ev) => {
      const id = ev.payload?.plan_id;
      if (id && ids.has(id) && !canvasFocusStore.isOpen()) canvasFocusStore.markPulse(id);
    }).then((fn) => {
      if (!active) fn();
      else unlisten = fn;
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [planIds]);

  const planOption: SegmentedOption<SwitchValue> = {
    value: "plan",
    label: ownsPlan ? "Plan" : "Tasks",
    icon: pulsing ? (
      <span
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: "var(--ct-accent, var(--ct-border-focus))",
          boxShadow: "0 0 0 3px color-mix(in srgb, var(--ct-accent, var(--ct-border-focus)) 30%, transparent)",
        }}
      />
    ) : undefined,
    title: ownsPlan ? "Show this orchestrator's plan canvas" : "Show your tasks on the canvas",
  };

  const baseOptions = defaultChatFirst ? [CHAT, TERMINAL] : [TERMINAL, CHAT];
  const options = planIds.length > 0 ? [...baseOptions, planOption] : baseOptions;
  const value: SwitchValue = planActive ? "plan" : mode;

  return (
    <Segmented
      data-testid="pane-mode-switch"
      value={value}
      options={options}
      height={height}
      onChange={(next) => {
        if (next === "plan") {
          for (const id of planIds) canvasFocusStore.clearPulse(id);
          setPanePlanMode(paneId, true);
          return;
        }
        setPanePlanMode(paneId, false);
        setMode(next);
        emitPaneUiMode(paneId, next);
      }}
    />
  );
}
