import { TerminalPane } from "./TerminalPane";
import { PaneSessionChat } from "./PaneSessionChat";
import { PaneTopBar } from "./PaneTopBar";
import { isPaneVisible } from "../MemoryPanel/workerFocusStore";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { useMultiplexer } from "../../store/multiplexer";
import { CanvasView } from "../Canvas/CanvasView";
import { setPanePlanMode, usePanePlanMode } from "../Canvas/panePlanMode";
import { resolveOrchestratorPaneId } from "../Canvas/planPaneMatch";
import type {
  ApiConfig,
  Pane,
  ThemeConfig,
  FontConfig,
  TerminalConfigSettings,
} from "../../lib/types";

interface TabContentProps {
  pane: Pane | undefined;
  isFocused: boolean;
  onFocusPane: (paneId: string) => void;
  onPtySpawned: (paneId: string, ptyId: string) => void;
  theme: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
  usageEnabledByProvider?: Record<string, boolean>;
  api?: ApiConfig;
  workspaceCwd?: string | null;
  uiFontScale?: number;
  isVisible: boolean;
  /** From App's single useConfig — chat is the default view (leads the picker). */
  chatFirst: boolean;
  /** From App's single useConfig — chat-default nudge already answered. */
  chatDefaultPrompted: boolean;
}

export function TabContent({
  pane,
  isFocused,
  onFocusPane,
  onPtySpawned,
  theme,
  fontConfig,
  terminalConfig,
  usageEnabledByProvider,
  workspaceCwd,
  uiFontScale,
  isVisible,
  chatFirst,
  chatDefaultPrompted,
  api,
}: TabContentProps) {
  const descriptor = useAgentDescriptor(pane?.agent_type);
  const { state } = useMultiplexer();
  const planActive = usePanePlanMode(pane?.id ?? "");

  if (!pane) return null;
  if (!isPaneVisible(pane)) return null;

  const isChatBackend = pane.backing?.kind === "chat_backend";
  // The switch only applies to PTY agent panes that can render both surfaces;
  // chatBackend panes are chat-only (no terminal), so they get pills but no switch.
  const showSwitch = !isChatBackend && descriptor?.supports_chat_ui === true;
  const usageProvider = !isChatBackend ? (pane.agent_type ?? null) : null;
  const usageVisible = !!usageProvider && usageEnabledByProvider?.[usageProvider] === true;
  const cwd = pane.cwd ?? workspaceCwd ?? null;

  const body = isChatBackend ? (
    <PaneSessionChat paneId={pane.id} chatBackend uiFontScale={uiFontScale} isVisible={isVisible} />
  ) : (
    <TerminalPane
      key={pane.id}
      paneId={pane.id}
      ptyId={pane.pty_id}
      isFocused={isFocused}
      isVisible={isVisible}
      hasSplits={false}
      onFocus={onFocusPane}
      onPtySpawned={onPtySpawned}
      theme={theme}
      fontConfig={fontConfig}
      terminalConfig={terminalConfig}
      uiFontScale={uiFontScale}
    />
  );

  return (
    <PaneWrapper paneId={pane.id}>
      <div className="flex h-full w-full flex-col">
        <PaneTopBar
          paneId={pane.id}
          cwd={cwd}
          showSwitch={showSwitch}
          chatFirst={chatFirst}
          chatDefaultPrompted={chatDefaultPrompted}
          usageProvider={usageProvider}
          usageVisible={usageVisible}
          api={api}
        />
        <div className="relative min-h-0 flex-1" style={{ background: theme.background }}>
          {body}
          {planActive && api && (
            <div className="absolute inset-0 z-20" data-testid="pane-plan-overlay">
              <CanvasView
                mode="focus"
                orchestratorPaneId={resolveOrchestratorPaneId(pane.id, state.panes)}
                focusPaneId={pane.id}
                api={api}
                onClose={() => setPanePlanMode(pane.id, false)}
              />
            </div>
          )}
        </div>
      </div>
    </PaneWrapper>
  );
}

interface PaneWrapperProps {
  paneId: string;
  children: React.ReactNode;
}

function PaneWrapper({ paneId, children }: PaneWrapperProps) {
  return (
    <div
      data-pane-id={paneId}
      className="w-full h-full relative group/pane"
      style={{ outline: "none", boxShadow: "none", border: "none", background: "transparent" }}
    >
      {children}
    </div>
  );
}
