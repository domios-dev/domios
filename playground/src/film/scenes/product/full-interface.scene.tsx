import "../../fixtures/supporter";
import type { CSSProperties } from "react";
import { Titlebar } from "../../../../../src/components/Titlebar/Titlebar";
import { UpdateProvider } from "../../../../../src/hooks/useUpdate";
import { UpdatePanelProvider } from "../../../../../src/hooks/useUpdatePanel";
import { DEFAULT_THEME } from "../../../../../src/lib/themeCss";
import { API } from "../../../gallery/fixtures";
import { draftAt, EVENTS, HISTORY, orchestratorAt, teamAt } from "../../fixtures/fullui";
import { OrchestratorPane, type ChatRow } from "../../fixtures/fullui-chat";
import { FullSidebar } from "../../fixtures/fullui-sidebar";
import { PhoneShell } from "../../fixtures/vertical-phone";
import { sidebarDrawerLayout } from "../../../../../src/components/Sidebar/sidebarDrawerLayout";
import { Camera, easeOut, range, useStage, type Scene } from "../../engine";

const W = 1536;
const H = 864;
// A standard 390 px phone: narrower widths wrap the product's card headers. The window ends above the Reels overlay.
const PHONE = { width: 390, height: 610, scale: 2.31 };
const DRAWER_CLOSE = 1.25;
const noop = () => undefined;
const HISTORY_ROWS: ChatRow[] = HISTORY.map((card, index) => ({ ...card, at: 0.55 + index * 0.15 }));

function arrive(t: number, at: number, dx: number, dy: number): CSSProperties {
  const k = range(t, at, at + 0.7, easeOut);
  return { opacity: 0.2 + 0.8 * k, transform: `translate(${(1 - k) * dx}px, ${(1 - k) * dy}px)` };
}

/** App.tsx's phone shell: the burger titlebar over a full-width chat, the Sidebar as an overlay drawer that slides away before the ask. */
function PhoneInterface({ t }: { t: number }) {
  const frame = teamAt(t);
  const { tab, status } = orchestratorAt(t);
  const rows = [...HISTORY_ROWS, ...EVENTS.filter((event) => t >= event.at)];
  const drawer = 1 - range(t, DRAWER_CLOSE, DRAWER_CLOSE + 0.35);
  const { width, height, scale } = PHONE;
  return (
    <Camera t={t} width={width} height={height} keys={[
      { t: 0, x: 195, y: 390, scale },
      { t: 8.6, x: 195, y: 390, scale },
      { t: 10, x: 195, y: 390, scale: scale + 0.02 },
    ]}>
      <PhoneShell>
        <div className="flex flex-col overflow-hidden" style={{ width, height, borderRadius: 28, boxShadow: "0 0 0 1px var(--ct-border)", background: "var(--ct-bg)", color: "var(--ct-fg)", "--ct-composer-bottom-padding": "calc(0.5rem + var(--ct-safe-bottom, 0px))" } as CSSProperties}>
          <div style={arrive(t, 0.05, 0, -18)}>
            <UpdateProvider><UpdatePanelProvider>
              <Titlebar api={API} theme={DEFAULT_THEME} showSidebarToggle onToggleSidebar={noop} onOpenSettings={noop} onOpenExtensions={noop} />
            </UpdatePanelProvider></UpdateProvider>
          </div>
          <div className="flex flex-1 min-h-0" data-testid="app-shell-body" data-shell-viewport="phone">
            <div className="flex flex-1 min-h-0 relative">
              {drawer > 0 && <>
                <div className="absolute inset-0 z-30 bg-black/40" style={{ opacity: drawer }} aria-hidden="true" />
                <div className="flex min-h-0" style={{ ...sidebarDrawerLayout(true), transform: `translateX(${(drawer - 1) * 100}%)` }}>
                  <div className="flex min-h-0" style={arrive(t, 0.2, -48, 0)}>
                    <FullSidebar frame={frame} />
                  </div>
                </div>
              </>}
              <div className="flex-1 min-w-0 min-h-0 flex flex-col" style={arrive(t, 0.4, 0, 28)}>
                <main className="flex-1 min-w-0 min-h-0 relative overflow-hidden">
                  <div className="absolute inset-0">
                    <OrchestratorPane t={t} tab={tab} status={status} rows={rows} draft={draftAt(t)} />
                  </div>
                </main>
              </div>
            </div>
          </div>
        </div>
      </PhoneShell>
    </Camera>
  );
}

function FullInterface({ t }: { t: number }) {
  if (useStage().portrait) return <PhoneInterface t={t} />;
  const frame = teamAt(t);
  const { tab, status } = orchestratorAt(t);
  const rows = [...HISTORY_ROWS, ...EVENTS.filter((event) => t >= event.at)];
  return (
    <Camera t={t} width={W} height={H} keys={[
      { t: 0, x: W / 2, y: H / 2, scale: 1.25 },
      { t: 8.6, x: W / 2, y: H / 2, scale: 1.25 },
      { t: 10, x: W / 2 + 12, y: H / 2 + 4, scale: 1.27 },
    ]}>
      <div className="flex flex-col overflow-hidden" style={{ width: W, height: H, background: "var(--ct-bg)", color: "var(--ct-fg)", "--ct-composer-bottom-padding": "calc(0.5rem + var(--ct-safe-bottom, 0px))" } as CSSProperties}>
        <div style={arrive(t, 0.05, 0, -18)}>
          <UpdateProvider><UpdatePanelProvider>
            <Titlebar api={API} theme={DEFAULT_THEME} onOpenSettings={noop} onOpenExtensions={noop} />
          </UpdatePanelProvider></UpdateProvider>
        </div>
        <div className="flex flex-1 min-h-0" data-testid="app-shell-body" data-shell-viewport="desktop">
          <div className="flex flex-1 min-h-0 relative">
            <div className="flex min-h-0" style={arrive(t, 0.2, -48, 0)}>
              <FullSidebar frame={frame} />
            </div>
            <div className="flex-1 min-w-0 min-h-0 flex flex-col" style={arrive(t, 0.4, 0, 28)}>
              <main className="flex-1 min-w-0 min-h-0 relative overflow-hidden">
                <div className="absolute inset-0">
                  <OrchestratorPane t={t} tab={tab} status={status} rows={rows} draft={draftAt(t)} />
                </div>
              </main>
            </div>
          </div>
        </div>
      </div>
    </Camera>
  );
}

const fullInterface: Scene = {
  id: "full-interface",
  duration: 10,
  render: (t) => <FullInterface t={t} />,
};

export default fullInterface;
