/**
 * WHAT: Agent loading, failure, and staffing cards remain legible in both themes.
 * WHY: Startup state must identify the affected role without obscuring recovery actions.
 * ORACLE: Accessible card content and visual snapshots match each state and theme.
 */
// Spawn failure maps to the starting view while `tab-boot-failed` owns recovery, so no failure card is rendered here.
import path from "node:path";
import { build } from "esbuild";
import { expect, test } from "../../helpers/fixtures";

const PROVIDER_NAME = "Codex";
const FAILURE_REASON = "fixture failure detail";
const startingStates = [
  ["spawn", "preparing_role_instructions"],
  ["fallback", "delivering_instructions"],
] as const;

/** Repo root — resolve the fixture's product imports against the checkout the
 *  spec lives in, never against whatever cwd the runner happened to start in.
 *  Playwright transpiles specs to CJS, so `import.meta.url` is not available. */
const repoRoot = path.resolve(__dirname, "../../../..");

/** One esbuild per FILE, not per test. Bundling the product twice inside each
 *  test's budget is what pushed this spec past its deadline and closed the page
 *  out from under it. */
let bundleOnce: Promise<string> | null = null;
const loadingCardBundle = (): Promise<string> => (bundleOnce ??= buildLoadingCardBundle());

async function buildLoadingCardBundle(): Promise<string> {
  const result = await build({
    bundle: true,
    format: "iife",
    platform: "browser",
    write: false,
    resolveExtensions: [".tsx", ".ts", ".jsx", ".js"],
    stdin: {
      resolveDir: repoRoot,
      sourcefile: "loading-card-visual-fixture.tsx",
      loader: "tsx",
      contents: `
        import React from "react";
        import { createRoot } from "react-dom/client";
        import { ChatSetupState, chatSetupView } from "./src/components/Terminal/ChatSetupState";
        import { TabBootOverlay, tabBootOverlayView } from "./src/components/Terminal/bootOverlay";
        import { TeamUnit } from "./src/components/Sidebar/TeamUnit";

        const colors = {
          dark: { bg: "#0d1117", fg: "#e6edf3", muted: "#8b949e", border: "#30363d", accent: "#58a6ff" },
          light: { bg: "#f6f8fa", fg: "#1f2328", muted: "#59636e", border: "#d0d7de", accent: "#0969da" },
        };

        const applyTheme = (theme) => {
          const palette = colors[theme];
          document.documentElement.style.colorScheme = theme;
          document.documentElement.style.setProperty("--ct-bg", palette.bg);
          document.documentElement.style.setProperty("--ct-fg", palette.fg);
          document.documentElement.style.setProperty("--ct-muted-fg", palette.muted);
          document.documentElement.style.setProperty("--ct-border", palette.border);
          document.documentElement.style.setProperty("--ct-accent", palette.accent);
          return palette;
        };

        const fixtureTab = (stage, failureReason) => ({
          id: "loading-card-fixture",
          type: "agent",
          title: "Planner",
          role: "Manager",
          role_profile: "planner",
          agent_type: "codex",
          agent_session_model: "gpt-5.6",
          cwd: "D:/Developer/codeterm",
          boot_stage: stage,
          boot_failure_reason: stage === "spawn_failed" ? failureReason : null,
          protected: true,
        });

        window.__renderLoadingCard = (stage, theme, providerName, failureReason) => {
          const palette = applyTheme(theme);
          document.body.innerHTML = '<div id="loading-card-root"></div>';
          const tab = fixtureTab(stage, failureReason);
          const frame = (children) => (
            <main className="flex h-full items-center justify-center">
              <div className="relative h-[420px] w-[640px] overflow-hidden border" style={{ borderColor: palette.border, background: palette.bg }}>
                {children}
              </div>
            </main>
          );
          const body = stage === "spawn_failed"
            ? (
              <TabBootOverlay
                view={tabBootOverlayView(tab)}
                shellReady={false}
                overlayVisible
                onRetry={() => undefined}
              />
            )
            : (
              <ChatSetupState
                tab={tab}
                view={chatSetupView(tab)}
                providerName={providerName}
                providerIconColor={palette.accent}
                providerIconHint="C"
              />
            );
          createRoot(document.getElementById("loading-card-root")).render(frame(body));
        };

        window.__renderTeamPicker = (theme) => {
          const palette = applyTheme(theme);
          document.body.innerHTML = '<div id="team-picker-root"></div>';
          createRoot(document.getElementById("team-picker-root")).render(
            <aside className="h-full w-[280px] p-3" style={{ background: palette.bg }}>
              <TeamUnit
                team={{ id: "team-picker-fixture", name: "Release 1.7.2" }}
                plan={null}
                tabs={{}}
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
  const readProgress = async () =>
    page
      .locator('[data-testid="chat-launch-ring"]')
      .evaluate((element) => (element as HTMLElement).style.getPropertyValue("--launch-progress"));

  try {
    for (const theme of ["dark", "light"] as const) {
      const progressByStage = new Map<string, number>();
      for (const [name, stage] of startingStates) {
        await page.goto(devInstance.webUrl);
        await page.addScriptTag({ content: bundle });
        await page.evaluate(
          ({ stage, theme, providerName, failureReason }) => {
            (window as typeof window & {
              __renderLoadingCard: (
                stage: string,
                theme: string,
                providerName: string,
                failureReason: string,
              ) => void;
            }).__renderLoadingCard(stage, theme, providerName, failureReason);
          },
          { stage, theme, providerName: PROVIDER_NAME, failureReason: FAILURE_REASON },
        );

        const card = page.locator('[data-testid="chat-setup-state"]');
        await expect(card).toBeVisible();
        // The provider identity the fixture supplied is the card's only name.
        await expect(card.locator('[data-testid="chat-launch-glyph"]'))
          .toHaveAttribute("aria-label", PROVIDER_NAME);
        const raw = await readProgress();
        const percent = Number.parseFloat(raw.replace("%", ""));
        expect(Number.isFinite(percent), `${name} ring exposes a numeric progress`).toBe(true);
        expect(percent).toBeGreaterThan(0);
        expect(percent).toBeLessThanOrEqual(100);
        progressByStage.set(stage, percent);

        await page.screenshot({
          path: testInfo.outputPath(`loading-card-${theme}-${name}.png`),
          fullPage: false,
        });
      }
      // The ring advances as boot advances — read off the product, not restated.
      expect(
        progressByStage.get("delivering_instructions"),
        "delivery sits further along the ring than instruction preparation",
      ).toBeGreaterThan(progressByStage.get("preparing_role_instructions")!);

      // spawn_failed leaves the chat overlay and surfaces on the boot overlay,
      // which owns the reason and the recovery action.
      await page.goto(devInstance.webUrl);
      await page.addScriptTag({ content: bundle });
      await page.evaluate(
        ({ theme, providerName, failureReason }) => {
          (window as typeof window & {
            __renderLoadingCard: (
              stage: string,
              theme: string,
              providerName: string,
              failureReason: string,
            ) => void;
          }).__renderLoadingCard("spawn_failed", theme, providerName, failureReason);
        },
        { theme, providerName: PROVIDER_NAME, failureReason: FAILURE_REASON },
      );
      const failed = page.locator('[data-testid="tab-boot-failed"]');
      await expect(failed).toBeVisible();
      await expect(failed).toContainText(FAILURE_REASON);
      await expect(failed.getByRole("button", { name: "Retry agent launch" })).toBeVisible();
      await expect(page.locator('[data-testid="chat-setup-state"]')).toHaveCount(0);
      await page.screenshot({
        path: testInfo.outputPath(`loading-card-${theme}-failure.png`),
        fullPage: false,
      });
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
