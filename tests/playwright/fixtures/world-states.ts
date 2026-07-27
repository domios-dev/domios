import type { Browser, Page } from "@playwright/test";
import type { ApiClient } from "../helpers/api-client";
import {
  seedPerfFixtureAndOpenInlineCanvas,
  CARD_METADATA_PROFILE_KEY,
} from "../helpers/canvas-inspect";

export const WORLD_STATE_PROFILE_KEY = CARD_METADATA_PROFILE_KEY;

export const worldStates = {
  planWithKnownTasks: "plan-with-known-tasks",
  tabTreeWithStatuses: "tab-tree-with-statuses",
  midStreamChatTranscript: "mid-stream-chat-transcript",
  doneHeavy: "done-heavy",
} as const;

export type CanvasWorldState = (typeof worldStates)[keyof typeof worldStates];

export type SeededCanvasWorld = {
  page: Page;
  workspaceId: string;
  worldState: CanvasWorldState;
  close: () => Promise<void>;
};

export async function seedCanvasWorldState(page: Page, worldState: CanvasWorldState): Promise<void> {
  await page.addInitScript(([profileKey, profile]) => {
    try {
      window.localStorage.setItem(String(profileKey), String(profile));
      window.localStorage.setItem("canvas.perfFixtureN", "1");
      window.localStorage.setItem("canvas.substrateV2", "true");
    } catch {
      /* surfaced by normal canvas wait helpers */
    }
  }, [WORLD_STATE_PROFILE_KEY, worldState]);
  await page.evaluate(
    ([profileKey, profile]) => {
      window.localStorage.setItem(String(profileKey), String(profile));
      window.localStorage.setItem("canvas.perfFixtureN", "1");
      window.localStorage.setItem("canvas.substrateV2", "true");
    },
    [WORLD_STATE_PROFILE_KEY, worldState],
  ).catch(() => {});
}

export async function seedWorldStateAndOpenInlineCanvas(
  browser: Browser,
  api: ApiClient,
  worldState: CanvasWorldState = worldStates.planWithKnownTasks,
): Promise<SeededCanvasWorld> {
  const canvas = await seedPerfFixtureAndOpenInlineCanvas(browser, api, 1, worldState);
  await canvas.page.waitForFunction(
    ([profileKey, expected]) => window.localStorage.getItem(String(profileKey)) === expected,
    [WORLD_STATE_PROFILE_KEY, worldState],
    { timeout: 30_000 },
  );
  await canvas.page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });
  return { ...canvas, worldState };
}

