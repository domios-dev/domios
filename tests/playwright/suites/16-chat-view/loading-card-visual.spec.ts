/**
 * WHAT: Agent loading, failure, and staffing cards remain legible in both themes.
 * WHY: Startup state must identify the affected role without obscuring recovery actions.
 * ORACLE: Accessible card content and visual snapshots match each state and theme.
 */
import { build } from "esbuild";
import { expect, test } from "../../helpers/fixtures";

const states = [
  ["spawn", "preparing_role_instructions", "Preparing planner instructions…"],
  ["fallback", "delivering_instructions", "Delivering instructions…"],
  ["failure", "spawn_failed", "Planner could not start"],
] as const;

async function loadingCardBundle(): Promise<string> {
  const result = await build({
    bundle: true,
    format: "iife",
    platform: "browser",
    write: false,
    resolveExtensions: [".tsx", ".ts", ".jsx", ".js"],
    stdin: {
      resolveDir: process.cwd(),
      sourcefile: "loading-card-visual-fixture.tsx",
      loader: "tsx",
      contents: `
        import React from "react";
        import { createRoot } from "react-dom/client";
        import { ChatSetupState, chatSetupView } from "./src/components/Terminal/ChatSetupState";
        import { TeamUnit } from "./src/components/Sidebar/TeamUnit";

        const colors = {
          dark: { bg: "#0d1117", fg: "#e6edf3", muted: "#8b949e", border: "#30363d", accent: "#58a6ff" },
          light: { bg: "#f6f8fa", fg: "#1f2328", muted: "#59636e", border: "#d0d7de", accent: "#0969da" },
        };

        window.__renderLoadingCard = (stage, theme) => {
          const palette = colors[theme];
          document.documentElement.style.colorScheme = theme;
          document.documentElement.style.setProperty("--ct-bg", palette.bg);
          document.documentElement.style.setProperty("--ct-fg", palette.fg);
          document.documentElement.style.setProperty("--ct-muted-fg", palette.muted);
          document.documentElement.style.setProperty("--ct-border", palette.border);
          document.documentElement.style.setProperty("--ct-accent", palette.accent);
          document.body.innerHTML = '<div id="loading-card-root"></div>';
          const pane = {
            id: "loading-card-fixture",
            type: "agent",
            title: "Planner",
            role: "Manager",
            role_profile: "planner",
            agent_type: "codex",
            agent_session_model: "gpt-5.6",
            cwd: "D:/Developer/codeterm",
            boot_stage: stage,
            boot_failure_reason: stage === "spawn_failed" ? "fixture failure detail" : null,
            protected: true,
          };
          createRoot(document.getElementById("loading-card-root")).render(
            <main className="flex h-full items-center justify-center">
              <div className="h-[420px] w-[640px] overflow-hidden border" style={{ borderColor: palette.border, background: palette.bg }}>
                <ChatSetupState
                  pane={pane}
                  view={chatSetupView(pane)}
                  providerName="Codex"
                  providerIconColor={palette.accent}
                  providerIconHint="C"
                  teamName="Release 1.7.2"
                  projectName="codeterm"
                  onRetry={() => undefined}
                  onOpenTerminal={() => undefined}
                />
              </div>
            </main>,
          );
        };

        window.__renderTeamPicker = (theme) => {
          const palette = colors[theme];
          document.documentElement.style.colorScheme = theme;
          document.documentElement.style.setProperty("--ct-bg", palette.bg);
          document.documentElement.style.setProperty("--ct-fg", palette.fg);
          document.documentElement.style.setProperty("--ct-muted-fg", palette.muted);
          document.documentElement.style.setProperty("--ct-border", palette.border);
          document.documentElement.style.setProperty("--ct-accent", palette.accent);
          document.body.innerHTML = '<div id="team-picker-root"></div>';
          createRoot(document.getElementById("team-picker-root")).render(
            <aside className="h-full w-[280px] p-3" style={{ background: palette.bg }}>
              <TeamUnit
                team={{ id: "team-picker-fixture", name: "Release 1.7.2" }}
                plan={null}
                panes={{}}
                managers={<div className="px-2 py-1 text-xs">Planner · Idle</div>}
                managerCount={1}
                workers={<div />}
                workerCount={0}
                workingWorkerCount={0}
                providers={[{
                  id: "fixture-provider",
                  display_name: "Fixture Provider",
                  transport: "pty",
                  icon_hint: "F",
                  icon_color: palette.accent,
                  behavior_config: { model: "required" },
                  default_model: "fixture-large",
                  available_models: [
                    { id: "fixture-large", display_name: "Fixture Large" },
                    { id: "fixture-fast", display_name: "Fixture Fast" },
                  ],
                }]}
                onOpenEffort={() => undefined}
                onAddMember={() => undefined}
                onCloseTeam={() => undefined}
              />
            </aside>,
          );
        };
      `,
    },
  });
  return result.outputFiles[0].text;
}

test("@golden loading identity card covers launch delivery and failure in both themes", async ({
  devInstance,
  webBrowser,
}, testInfo) => {
  const bundle = await loadingCardBundle();
  const page = await webBrowser.newPage({ viewport: { width: 900, height: 620 } });
  try {
    for (const theme of ["dark", "light"] as const) {
      for (const [name, stage, expected] of states) {
        await page.goto(devInstance.webUrl);
        await page.addScriptTag({ content: bundle });
        await page.evaluate(
          ({ stage, theme }) => {
            const render = (window as typeof window & {
              __renderLoadingCard: (stage: string, theme: string) => void;
            }).__renderLoadingCard;
            render(stage, theme);
          },
          { stage, theme },
        );
        const card = page.locator(
          stage === "spawn_failed" ? '[data-testid="chat-launch-failed"]' : '[data-testid="chat-setup-state"]',
        );
        await expect(card).toBeVisible();
        await expect(card).toContainText(expected);
        await page.screenshot({
          path: testInfo.outputPath(`loading-card-${theme}-${name}.png`),
          fullPage: false,
        });
      }
    }
  } finally {
    await page.close();
  }
});

test("@golden staffing picker shows provider and model steps in both themes", async ({
  devInstance,
  webBrowser,
}, testInfo) => {
  const bundle = await loadingCardBundle();
  const page = await webBrowser.newPage({ viewport: { width: 900, height: 620 } });
  try {
    for (const theme of ["dark", "light"] as const) {
      await page.goto(devInstance.webUrl);
      await page.addScriptTag({ content: bundle });
      await page.evaluate((theme) => {
        const render = (window as typeof window & {
          __renderTeamPicker: (theme: string) => void;
        }).__renderTeamPicker;
        render(theme);
      }, theme);
      await page.getByTestId("team-menu-trigger").focus();
      await page.getByTestId("team-menu-trigger").press("Enter");
      await page.getByRole("button", { name: "Add worker" }).click();
      await expect(page.getByText("Choose agent")).toBeVisible();
      await page.screenshot({
        path: testInfo.outputPath(`staffing-picker-${theme}-provider.png`),
        fullPage: false,
      });
      await page.getByRole("button", { name: "Fixture Provider" }).click();
      await expect(page.getByText("Choose model")).toBeVisible();
      await expect(page.getByTestId("team-picker-model-fixture-large")).toHaveAttribute("aria-checked", "true");
      await page.screenshot({
        path: testInfo.outputPath(`staffing-picker-${theme}-model.png`),
        fullPage: false,
      });
    }
  } finally {
    await page.close();
  }
});
